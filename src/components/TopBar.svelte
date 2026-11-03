<script lang="ts">
  import { onMount } from "svelte";
  import {
    RadioTower,
    Wifi,
    BookOpen,
    BookX,
    CircleCheck,
    CircleAlert,
    LoaderCircle,
    FileClock,
    Lock,
  } from "@lucide/svelte";
  import { app } from "../lib/state.svelte";
  import { formatClock, formatInstant } from "../lib/time";

  let now = $state(new Date());
  onMount(() => {
    const t = setInterval(() => (now = new Date()), 1000);
    return () => clearInterval(t);
  });
  const weekday = (d: Date, utc: boolean) =>
    d.toLocaleDateString("en-US", { weekday: "short", timeZone: utc ? "UTC" : undefined });
</script>

<header class="topbar">
  <div class="brand">
    <RadioTower size={30} strokeWidth={1.8} aria-hidden="true" />
    <span class="title">EMCOMM LOG <span class="v">V2</span></span>
  </div>
  <div class="center">
    <span class="pill offline" title="All features work without a network connection"
      ><Wifi size={16} aria-hidden="true" />OFFLINE READY</span
    >
    {#if app.active}
      {#if app.readOnly}
        <span class="pill archived"><Lock size={16} aria-hidden="true" />ARCHIVED (READ-ONLY)</span>
      {:else}
        <span class="pill open"><BookOpen size={16} aria-hidden="true" />LOGBOOK OPEN</span>
      {/if}
    {:else}
      <span class="pill closed"><BookX size={16} aria-hidden="true" />NO LOGBOOK OPEN</span>
    {/if}
    {#if app.active}
      {@const t = (iso: string) => formatInstant(iso, app.zone, app.clock, { date: false })}
      {#if app.saveStatus.kind === "saving"}
        <span class="pill saving"><LoaderCircle size={16} class="spin" aria-hidden="true" />SAVING…</span>
      {:else if app.saveStatus.kind === "failed"}
        <span class="pill failed" title={app.saveStatus.error.message}
          ><CircleAlert size={16} aria-hidden="true" />SAVE FAILED</span
        >
      {:else if app.saveStatus.kind === "invalid"}
        <span class="pill invalid" title={app.saveStatus.message}
          ><CircleAlert size={16} aria-hidden="true" />NOT SAVED · CHECK FIELDS</span
        >
      {:else if app.draftStatus.kind === "failed"}
        <span class="pill failed" title={app.draftStatus.error.message}
          ><CircleAlert size={16} aria-hidden="true" />DRAFT SAVE FAILED</span
        >
      {:else if app.editorDirty && app.draftStatus.kind === "saved"}
        <span
          class="pill draft"
          title="Unfinished entry stored in the local recovery area — not a logbook entry yet"
          ><FileClock size={16} aria-hidden="true" />DRAFT SAVED {t(app.draftStatus.at)}</span
        >
      {:else if app.editorDirty}
        <span class="pill draft"><FileClock size={16} aria-hidden="true" />UNSAVED ENTRY</span>
      {:else if app.saveStatus.kind === "saved"}
        <span class="pill saved" title="Committed to the live logbook database"
          ><CircleCheck size={16} aria-hidden="true" />{app.saveStatus.edit ? "CHANGES SAVED" : "ENTRY SAVED"} #{app
            .saveStatus.seq} · {t(app.saveStatus.at)}</span
        >
      {/if}
    {/if}
  </div>
  <div class="clocks" aria-label="Current time">
    <span
      ><span class="muted">Local:</span> {weekday(now, false)}, {formatClock(now, "local", app.clock)}</span
    >
    <span><span class="muted">UTC:</span> {weekday(now, true)}, {formatClock(now, "utc", app.clock)}</span>
  </div>
</header>

<style>
  .topbar {
    display: flex;
    align-items: center;
    gap: 16px;
    height: 60px;
    padding-right: 20px;
    background: var(--bg-sidebar);
    border-bottom: 1px solid var(--border);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 100%;
    padding: 0 34px 0 20px;
    color: var(--cyan);
    background: linear-gradient(90deg, var(--panel-2), var(--panel-3));
    clip-path: polygon(0 0, 100% 0, calc(100% - 22px) 100%, 0 100%);
    border-right: 2px solid var(--accent);
    min-width: 240px;
  }
  .title {
    font-size: 21px;
    font-weight: 800;
    color: var(--text);
    letter-spacing: 0.5px;
  }
  .v {
    color: var(--cyan);
  }
  .center {
    flex: 1;
    display: flex;
    gap: 10px;
    justify-content: center;
    flex-wrap: nowrap;
    overflow: hidden;
    min-width: 0;
    align-items: center;
  }
  .offline {
    color: var(--ok);
    border-color: var(--ok);
    background: var(--ok-bg);
  }
  .open {
    color: var(--cyan);
    border-color: var(--cyan);
    background: var(--info-bg);
  }
  .closed,
  .archived {
    color: var(--muted);
    border-color: var(--border-strong);
  }
  .saved {
    color: var(--ok);
    border-color: var(--ok);
  }
  .saving {
    color: var(--text-2);
    border-color: var(--border-strong);
  }
  .failed {
    color: var(--danger);
    border-color: var(--danger);
    background: var(--danger-bg);
  }
  .draft,
  .invalid {
    color: var(--warn);
    border-color: var(--warn);
  }
  .clocks {
    display: flex;
    gap: 22px;
    font-size: 14px;
    white-space: nowrap;
  }
  @media (max-width: 1350px) {
    .clocks {
      flex-direction: column;
      gap: 0;
      font-size: 12.5px;
    }
  }
</style>
