<script lang="ts">
  import { onMount } from "svelte";
  import { Zap, Trash2, Star, Copy, Eye } from "@lucide/svelte";
  import { app, defaultQuery } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import { confirm } from "../lib/confirm.svelte";
  import type * as T from "../lib/types";
  import { allowedKinds, applyTemplate, formCopyOf, KIND_LABEL, newForm } from "../lib/entryForm";
  import type { EntryForm } from "../lib/entryForm";
  import { formatInstant } from "../lib/time";
  import { currentPeriod } from "../lib/periods";
  import EntryEditor from "../components/EntryEditor.svelte";
  import OperatorBar from "../components/OperatorBar.svelte";
  import KindBadge from "../components/KindBadge.svelte";

  let tab = $state<"entry" | "templates" | "favorites">("entry");
  let templates = $state<T.EntryTemplate[]>([]);
  let favorites = $state<T.Entry[]>([]);
  let editorKey = $state(0);
  let initial = $state<EntryForm | null>(null);
  let initialDraft = $state<T.Draft | null>(null);

  const book = app.active!;
  const type = book.info.logbook_type;
  const ctx = () => ({
    type,
    preset: book.settings.preset,
    profile: app.profile,
    incident: app.active?.info.incident ?? book.info.incident,
    currentOpId: type === "emergency" ? (currentPeriod(app.periods)?.id ?? null) : null,
  });
  const op = $derived(type === "emergency" ? currentPeriod(app.periods) : null);

  async function loadTemplates() {
    templates = await api.listTemplates().catch(() => []);
  }
  async function loadFavorites() {
    favorites = (
      await api
        .listEntries({ ...defaultQuery(), favorites_only: true, limit: 200 })
        .catch(() => ({ items: [] as T.Entry[] }))
    ).items;
  }
  onMount(() => {
    loadTemplates();
    loadFavorites();
  });

  // Requests from shortcuts, the dashboard, favorites and draft recovery.
  $effect(() => {
    const r = app.editorRequest;
    if (!r) return;
    app.editorRequest = null;
    tab = "entry";
    if (app.editorDirty && r.mode !== "draft") {
      // Keep the operator's unsaved text: a new request never silently replaces it.
      if (r.mode !== "new") {
        confirm
          .ask({
            title: "Replace the current form?",
            message:
              "The entry you are typing will be kept as a recoverable draft, and the form will be replaced.",
            buttons: [
              { id: "ok", label: "Replace", tone: "primary" },
              { id: "cancel", label: "Keep typing" },
            ],
            cancelId: "cancel",
          })
          .then(async (a) => {
            if (a.id === "ok") {
              await app.flushDrafts();
              load(r);
            }
          });
      }
      return;
    }
    load(r);
  });

  function load(r: NonNullable<typeof app.editorRequest>) {
    initialDraft = null;
    initial = null;
    if (r.mode === "template")
      initial = applyTemplate(newForm(ctx()), r.template.defaults, allowedKinds(type));
    else if (r.mode === "copy") initial = formCopyOf(r.entry, ctx());
    else if (r.mode === "draft") initialDraft = r.draft;
    editorKey++;
  }

  async function deleteTemplate(t: T.EntryTemplate) {
    const a = await confirm.ask({
      title: "Delete template?",
      message: `"${t.name}" will be removed from this logbook. Entries created from it are not affected.`,
      buttons: [
        { id: "del", label: "Delete", tone: "danger" },
        { id: "cancel", label: "Cancel", tone: "primary" },
      ],
      cancelId: "cancel",
    });
    if (a.id !== "del") return;
    try {
      await api.deleteTemplate(t.id);
      await loadTemplates();
      app.toast("ok", "Template deleted");
    } catch (e) {
      app.toast("error", toAppError(e).message);
    }
  }

  async function renameTemplate(t: T.EntryTemplate) {
    const a = await confirm.ask({
      title: "Rename template",
      message: "Change the template's name.",
      input: { label: "Name", value: t.name, maxLength: 80 },
      buttons: [
        { id: "ok", label: "Rename", tone: "primary" },
        { id: "cancel", label: "Cancel" },
      ],
      cancelId: "cancel",
    });
    if (a.id !== "ok" || !a.value.trim()) return;
    try {
      await api.saveTemplate(t.id, a.value.trim(), t.defaults);
      await loadTemplates();
    } catch (e) {
      app.toast("error", toAppError(e).message);
    }
  }

  function selectTab(t: typeof tab) {
    tab = t;
    if (t === "favorites") loadFavorites();
    if (t === "templates") loadTemplates();
  }

  function onTabKey(e: KeyboardEvent) {
    const order: (typeof tab)[] = ["entry", "templates", "favorites"];
    const i = order.indexOf(tab);
    if (e.key === "ArrowRight") selectTab(order[(i + 1) % 3]);
    else if (e.key === "ArrowLeft") selectTab(order[(i + 2) % 3]);
    else return;
    e.preventDefault();
    document.getElementById(`qe-tab-${tab}`)?.focus();
  }
</script>

<section class="panel">
  <div class="panel-head">
    <h1 class="title">
      <Zap size={22} class="icon" />Quick Entry – Add {type === "ham" ? "Log Entry" : "Traffic"}
    </h1>
    <span class="spacer"></span>
    {#if op}<span class="badge ok" title="New entries are tagged with this operational period"
        >PERIOD: {op.name}</span
      >{:else if type === "emergency" && app.periods.some((p) => p.status === "active")}<span
        class="badge warn"
        title="No operational period is running now. New entries are placed by their time; add the next period on the Dashboard."
        >NO CURRENT PERIOD</span
      >{/if}
    <span class="muted"
      >Times entered in {app.zone === "utc" ? "UTC" : "local time"} · {app.stats?.total_active ?? 0} active entries</span
    >
  </div>
  <div class="opbar-row"><OperatorBar compact /></div>
  <div class="tabs" role="tablist" aria-label="Quick Entry" tabindex="-1" onkeydown={onTabKey}>
    <button
      id="qe-tab-entry"
      role="tab"
      aria-selected={tab === "entry"}
      aria-controls="qe-panel"
      tabindex={tab === "entry" ? 0 : -1}
      onclick={() => selectTab("entry")}>{type === "ham" ? "Log Entry" : "Traffic Entry"}</button
    >
    <button
      id="qe-tab-templates"
      role="tab"
      aria-selected={tab === "templates"}
      aria-controls="qe-panel"
      tabindex={tab === "templates" ? 0 : -1}
      onclick={() => selectTab("templates")}>Templates ({templates.length})</button
    >
    <button
      id="qe-tab-favorites"
      role="tab"
      aria-selected={tab === "favorites"}
      aria-controls="qe-panel"
      tabindex={tab === "favorites" ? 0 : -1}
      onclick={() => selectTab("favorites")}><Star size={15} />Favorites</button
    >
  </div>
  <div class="panel-body" id="qe-panel" role="tabpanel" aria-labelledby="qe-tab-{tab}">
    <div hidden={tab !== "entry"}>
      {#key editorKey}
        <EntryEditor mode="new" {initial} {initialDraft} {templates} ontemplatesaved={loadTemplates} />
      {/key}
    </div>
    {#if tab === "templates"}
      {#if templates.length === 0}
        <p class="empty">
          No templates in this logbook yet. Fill in the entry form and choose <strong>Save as Template</strong
          >.
        </p>
      {:else}
        <table class="data">
          <thead
            ><tr
              ><th>Name</th><th>Type</th><th>Summary</th><th>Version</th><th
                ><span class="sr-only">Actions</span></th
              ></tr
            ></thead
          >
          <tbody>
            {#each templates as t (t.id)}
              <tr>
                <td><strong>{t.name}</strong></td>
                <td>{t.defaults.kind ? KIND_LABEL[t.defaults.kind] : "—"}</td>
                <td class="ellipsis" style="max-width:380px">{t.defaults.subject}</td>
                <td>v{t.version}</td>
                <td class="row" style="gap:6px">
                  <button
                    class="small primary"
                    onclick={() => (app.editorRequest = { mode: "template", template: t })}>Use</button
                  >
                  <button class="small" onclick={() => renameTemplate(t)}>Rename</button>
                  <button
                    class="small icon-only"
                    aria-label="Delete template {t.name}"
                    onclick={() => deleteTemplate(t)}><Trash2 size={15} /></button
                  >
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    {:else if tab === "favorites"}
      {#if favorites.length === 0}
        <p class="empty">
          No favorite entries. Mark entries with the star in the Traffic Log or entry details.
        </p>
      {:else}
        <table class="data">
          <thead
            ><tr
              ><th>#</th><th>Time</th><th>Type</th><th>From → To</th><th>Summary</th><th
                ><span class="sr-only">Actions</span></th
              ></tr
            ></thead
          >
          <tbody>
            {#each favorites as e (e.id)}
              <tr>
                <td class="mono">{String(e.sequence_no).padStart(4, "0")}</td>
                <td class="nowrap">{formatInstant(e.event_at_utc, app.zone, app.clock)}</td>
                <td><KindBadge kind={e.kind} /></td>
                <td>{e.from_text || "—"} → {e.to_text || "—"}</td>
                <td class="ellipsis" style="max-width:380px">{e.subject}</td>
                <td class="row" style="gap:6px">
                  <button
                    class="small primary"
                    onclick={() => (app.editorRequest = { mode: "copy", entry: e })}
                    ><Copy size={14} />Use as new entry</button
                  >
                  <button
                    class="small icon-only"
                    aria-label="View entry {e.sequence_no}"
                    onclick={() => (app.detailEntryId = e.id)}><Eye size={15} /></button
                  >
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    {/if}
  </div>
</section>

<style>
  .opbar-row {
    padding: 6px 16px 0;
  }
  .title {
    display: flex;
    gap: 10px;
    align-items: center;
    font-size: 20px;
  }
</style>
