<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { app } from "./lib/state.svelte";
  import { api, toAppError } from "./lib/api";
  import { runAction, keyToAction, handleCloseRequested } from "./lib/actions";
  import type { BackupStatus } from "./lib/types";
  import TopBar from "./components/TopBar.svelte";
  import Sidebar from "./components/Sidebar.svelte";
  import Toasts from "./components/Toasts.svelte";
  import ConfirmHost from "./components/ConfirmHost.svelte";
  import HelpDialogs from "./components/HelpDialogs.svelte";
  import DraftRecovery from "./components/DraftRecovery.svelte";
  import Wizard from "./screens/Wizard.svelte";
  import Home from "./screens/Home.svelte";
  import Dashboard from "./screens/Dashboard.svelte";
  import QuickEntry from "./screens/QuickEntry.svelte";
  import TrafficLog from "./screens/TrafficLog.svelte";
  import Messages from "./screens/Messages.svelte";
  import Reports from "./screens/Reports.svelte";
  import Settings from "./screens/Settings.svelte";
  import EntryDetail from "./components/EntryDetail.svelte";

  let loadError = $state<string | null>(null);
  let loaded = $state(false);

  onMount(() => {
    const unlisten: (() => void)[] = [];
    (async () => {
      try {
        app.applyBootstrap(await api.getBootstrap());
        app.screen = !app.settings?.wizard_completed ? "wizard" : app.active ? "dashboard" : "home";
        loaded = true;
      } catch (e) {
        loadError = toAppError(e).message;
      }
      unlisten.push(await listen<string>("menu", (ev) => runAction(ev.payload, "menu")));
      unlisten.push(await listen("close-requested", () => handleCloseRequested()));
      unlisten.push(
        await listen<BackupStatus>("backup-status", (ev) => {
          const prev = app.backup?.last;
          app.backup = ev.payload;
          const last = ev.payload.last;
          if (
            last &&
            !ev.payload.running &&
            last.finished_at_utc !== prev?.finished_at_utc &&
            last.kind === "auto"
          ) {
            if (last.ok) app.announce("Automatic backup succeeded");
            else app.toast("error", `Automatic Backup Failed: ${last.message}`);
          }
        }),
      );
    })();
    const onKey = (e: KeyboardEvent) => {
      const a = keyToAction(e);
      if (a) {
        e.preventDefault();
        runAction(a, "key");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      unlisten.forEach((u) => u());
    };
  });

  // Theme follows settings; "system" tracks the OS preference.
  $effect(() => {
    const t = app.settings?.theme ?? "dark";
    const apply = () => {
      const dark = t === "dark" || (t === "system" && matchMedia("(prefers-color-scheme: dark)").matches);
      document.documentElement.dataset.theme = dark ? "dark" : "light";
    };
    apply();
    if (t === "system") {
      const mq = matchMedia("(prefers-color-scheme: dark)");
      mq.addEventListener("change", apply);
      return () => mq.removeEventListener("change", apply);
    }
  });

  // Stats follow data changes and the open logbook.
  $effect(() => {
    void app.dataVersion;
    void app.zone;
    app.refreshStats();
  });

  // When a logbook opens: surface integrity notices and any recoverable drafts.
  let lastBook: string | null = null;
  $effect(() => {
    const id = app.active?.info.id ?? null;
    if (id && id !== lastBook) {
      for (const n of app.active?.notices ?? []) app.toast("warn", n, 12000);
      if (app.active?.drafts.length) {
        app.pendingRecovery = app.active.drafts;
        app.dialog = "recovery";
      }
    }
    lastBook = id;
  });
</script>

{#if loadError}
  <main class="fatal" role="alert">
    <h1>EMCOMM LOG could not start</h1>
    <p>{loadError}</p>
    <p class="muted">
      Your logbooks were not modified. Check that the app data folder is writable, then restart.
    </p>
  </main>
{:else if !loaded}
  <main class="fatal" aria-busy="true"><p class="muted">Loading…</p></main>
{:else if app.screen === "wizard"}
  <Wizard />
{:else}
  <div class="shell">
    <TopBar />
    <div class="body">
      <Sidebar />
      <main class="content" id="main">
        {#if app.screen === "home"}
          <Home />
        {:else if app.screen === "settings"}
          <Settings />
        {:else if !app.active}
          <Home />
        {:else if app.screen === "dashboard"}
          <Dashboard />
        {:else if app.screen === "quick"}
          <QuickEntry />
        {:else if app.screen === "log"}
          <TrafficLog />
        {:else if app.screen === "messages"}
          <Messages />
        {:else if app.screen === "reports"}
          <Reports />
        {/if}
      </main>
    </div>
  </div>
  {#if app.detailEntryId}
    <EntryDetail />
  {/if}
  {#if app.dialog === "recovery"}
    <DraftRecovery />
  {/if}
  <HelpDialogs />
{/if}
<ConfirmHost />
<Toasts />

<style>
  .shell {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }
  .body {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  .content {
    flex: 1;
    min-width: 0;
    overflow: auto;
    padding: 18px 20px;
  }
  .fatal {
    max-width: 640px;
    margin: 10vh auto;
    padding: 24px;
  }
</style>
