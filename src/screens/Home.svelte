<script lang="ts">
  import { onMount } from "svelte";
  import {
    BookPlus,
    FolderOpen,
    PackageOpen,
    ArchiveRestore,
    Settings as SettingsIcon,
    TriangleAlert,
    Archive,
    PencilLine,
    PackagePlus,
    FolderSearch,
    Link,
    Trash2,
    BookOpen,
  } from "@lucide/svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import { closeActiveLogbook } from "../lib/actions";
  import { confirm } from "../lib/confirm.svelte";
  import type { LogbookIndexRow } from "../lib/types";
  import { TYPE_LABEL } from "../lib/entryForm";
  import { formatInstant, relativeAge } from "../lib/time";
  import CreateLogbook from "../components/CreateLogbook.svelte";
  import RestoreDialog from "../components/RestoreDialog.svelte";
  import ImportDialog from "../components/ImportDialog.svelte";

  let rows = $state<LogbookIndexRow[]>([]);
  let showArchived = $state(false);
  let busyId = $state<string | null>(null);
  let loadErr = $state<string | null>(null);

  async function load() {
    try {
      rows = await api.listLogbooks();
      loadErr = null;
    } catch (e) {
      loadErr = toAppError(e).message;
    }
  }
  onMount(load);
  $effect(() => {
    void app.dataVersion;
    load();
  });

  const shown = $derived(rows.filter((r) => showArchived || !r.archived));
  const archivedCount = $derived(rows.filter((r) => r.archived).length);

  async function guard(id: string, f: () => Promise<void>) {
    busyId = id;
    try {
      await f();
    } catch (e) {
      app.toast("error", toAppError(e).message);
    } finally {
      busyId = null;
    }
  }

  async function open(r: LogbookIndexRow) {
    if (app.active?.info.id === r.id) {
      app.screen = "dashboard";
      return;
    }
    if (app.active && !(await closeActiveLogbook())) return;
    await guard(r.id, async () => {
      app.setActive(await api.openLogbook(r.id));
      app.saveStatus = { kind: "idle" };
      app.screen = "dashboard";
      app.toast("ok", `Opened ${r.name}`);
    });
    await load();
  }

  async function openFolder() {
    const p = await api.pickFolder("Open an existing logbook folder", app.settings?.default_logbook_parent);
    if (!p) return;
    if (app.active && !(await closeActiveLogbook())) return;
    await guard("folder", async () => {
      app.setActive(await api.openLogbookFolder(p));
      app.screen = "dashboard";
    });
    await load();
  }

  async function rename(r: LogbookIndexRow) {
    const a = await confirm.ask({
      title: "Rename logbook",
      message: "The folder on disk keeps its name; only the logbook's display name changes.",
      input: { label: "New name", value: r.name, maxLength: 120 },
      buttons: [
        { id: "ok", label: "Rename", tone: "primary" },
        { id: "cancel", label: "Cancel" },
      ],
      cancelId: "cancel",
    });
    if (a.id !== "ok" || !a.value.trim() || a.value === r.name) return;
    await guard(r.id, async () => {
      rows = await api.renameLogbook(r.id, a.value.trim());
      if (app.active?.info.id === r.id) await app.refreshActive();
    });
  }

  async function archive(r: LogbookIndexRow) {
    if (!r.archived) {
      const a = await confirm.ask({
        title: "Archive logbook?",
        message: `"${r.name}" becomes read-only and is hidden from the default list. No files are moved or deleted; you can unarchive it any time.`,
        buttons: [
          { id: "ok", label: "Archive", tone: "primary" },
          { id: "cancel", label: "Cancel" },
        ],
        cancelId: "cancel",
      });
      if (a.id !== "ok") return;
    }
    await guard(r.id, async () => {
      rows = await api.setLogbookArchived(r.id, !r.archived);
      if (app.active?.info.id === r.id) await app.refreshActive();
      app.toast("ok", r.archived ? "Unarchived — editing enabled" : "Archived (read-only)");
    });
  }

  async function exportPkg(r: LogbookIndexRow) {
    const dest = await api.pickSaveFile("Export portable logbook package", `${r.name}.emlog`, [
      { name: "EMCOMM LOG package", extensions: ["emlog"] },
    ]);
    if (!dest) return;
    await guard(r.id, async () => {
      const res = await api.exportPackage(r.id, dest);
      app.toast(
        "ok",
        `Package exported: ${res.entry_count} entries, ${res.attachment_count} attachment(s), verified.`,
      );
    });
  }

  async function relink(r: LogbookIndexRow) {
    const p = await api.pickFolder(`Locate the folder for "${r.name}"`, null);
    if (!p) return;
    await guard(r.id, async () => {
      rows = await api.relinkLogbook(r.id, p);
      app.toast("ok", "Logbook relinked");
    });
  }

  async function forget(r: LogbookIndexRow) {
    const a = await confirm.ask({
      title: "Remove from list?",
      message: `"${r.name}" will be removed from this computer's logbook list only. Its folder and data are NOT deleted.`,
      detail: r.path,
      buttons: [
        { id: "ok", label: "Remove from list", tone: "danger" },
        { id: "cancel", label: "Cancel", tone: "primary" },
      ],
      cancelId: "cancel",
    });
    if (a.id !== "ok") return;
    await guard(r.id, async () => {
      rows = await api.forgetLogbook(r.id);
    });
  }

  function range(r: LogbookIndexRow) {
    if (!r.first_event_utc) return "—";
    const a = formatInstant(r.first_event_utc, app.zone, app.clock, { zone: false }).slice(0, 10);
    const b = formatInstant(r.last_event_utc, app.zone, app.clock, { zone: false }).slice(0, 10);
    return a === b ? a : `${a} → ${b}`;
  }

  async function afterCreatedOrImported() {
    await load();
  }
</script>

<div class="page">
  <header class="row wrap head">
    <h1>Logbooks</h1>
    <span class="spacer"></span>
    <button class="primary" onclick={() => (app.dialog = "create")}
      ><BookPlus size={17} />Create Logbook</button
    >
    <button onclick={openFolder}><FolderOpen size={17} />Open Folder…</button>
    <button onclick={() => (app.dialog = "import")}><PackageOpen size={17} />Import Package…</button>
    <button onclick={() => (app.dialog = "restore")}><ArchiveRestore size={17} />Restore Backup…</button>
    <button onclick={() => app.navigate("settings")}><SettingsIcon size={17} />Settings</button>
  </header>

  {#if loadErr}<div class="banner error" role="alert">
      <TriangleAlert size={18} class="icon" />{loadErr}
    </div>{/if}

  <section class="panel">
    <div class="panel-head">
      <h2><BookOpen size={20} class="icon" />Local logbooks</h2>
      <span class="spacer"></span>
      {#if archivedCount}<label class="check"
          ><input type="checkbox" bind:checked={showArchived} /> Show archived ({archivedCount})</label
        >{/if}
    </div>
    {#if shown.length === 0}
      <div class="empty">
        <p>No logbooks yet.</p>
        <button class="primary" onclick={() => (app.dialog = "create")}
          ><BookPlus size={17} />Create your first logbook</button
        >
      </div>
    {:else}
      <div style="overflow:auto">
        <table class="data">
          <thead>
            <tr>
              <th scope="col">Name</th>
              <th scope="col">Type</th>
              <th scope="col">Incident / Location</th>
              <th scope="col">Event dates</th>
              <th scope="col">Last modified</th>
              <th scope="col">Entries</th>
              <th scope="col">Location on disk</th>
              <th scope="col"><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {#each shown as r (r.id)}
              <tr aria-busy={busyId === r.id}>
                <td>
                  <button class="link name" onclick={() => open(r)} disabled={!r.available || busyId !== null}
                    >{r.name}</button
                  >
                  <div class="row" style="gap:6px;margin-top:3px">
                    {#if app.active?.info.id === r.id}<span class="badge ok">OPEN</span>{/if}
                    {#if r.archived}<span class="badge neutral">ARCHIVED</span>{/if}
                    {#if !r.available}<span class="badge danger">UNAVAILABLE</span>{/if}
                    {#if r.source_uuid}<span
                        class="badge info"
                        title="Imported or restored from {r.source_uuid}">COPY</span
                      >{/if}
                  </div>
                </td>
                <td>{TYPE_LABEL[r.logbook_type]}</td>
                <td>{r.incident_name || r.location || "—"}</td>
                <td class="nowrap">{range(r)}</td>
                <td class="nowrap" title={r.last_modified_utc ?? ""}>{relativeAge(r.last_modified_utc)}</td>
                <td>{r.entry_count}</td>
                <td class="mono path" title={r.path}>{r.path}</td>
                <td>
                  <div class="row actions">
                    {#if r.available}
                      <button class="small primary" onclick={() => open(r)} disabled={busyId !== null}
                        >{app.active?.info.id === r.id ? "Go to" : "Open"}</button
                      >
                      <button
                        class="small icon-only"
                        title="Rename"
                        aria-label="Rename {r.name}"
                        onclick={() => rename(r)}
                        disabled={busyId !== null}><PencilLine size={15} /></button
                      >
                      <button
                        class="small icon-only"
                        title={r.archived ? "Unarchive" : "Archive"}
                        aria-label="{r.archived ? 'Unarchive' : 'Archive'} {r.name}"
                        onclick={() => archive(r)}
                        disabled={busyId !== null}><Archive size={15} /></button
                      >
                      <button
                        class="small icon-only"
                        title="Export package (.emlog)"
                        aria-label="Export package for {r.name}"
                        onclick={() => exportPkg(r)}
                        disabled={busyId !== null}><PackagePlus size={15} /></button
                      >
                      <button
                        class="small icon-only"
                        title="Reveal location"
                        aria-label="Reveal location of {r.name}"
                        onclick={() =>
                          api.revealPath(r.path).catch((e) => app.toast("error", toAppError(e).message))}
                        ><FolderSearch size={15} /></button
                      >
                    {:else}
                      <button class="small" onclick={() => relink(r)}><Link size={15} />Relink…</button>
                      <button class="small" onclick={load}>Retry</button>
                    {/if}
                    {#if app.active?.info.id !== r.id}
                      <button
                        class="small icon-only"
                        title="Remove from list (keeps files)"
                        aria-label="Remove {r.name} from list"
                        onclick={() => forget(r)}
                        disabled={busyId !== null}><Trash2 size={15} /></button
                      >
                    {/if}
                  </div>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </section>
  <p class="muted small">
    Unavailable means the folder could not be found (for example, a removable drive is unplugged). EMCOMM LOG
    never creates a replacement elsewhere — reconnect the drive and press Retry, or Relink to the folder's new
    location.
  </p>
</div>

{#if app.dialog === "create"}
  <CreateLogbook onclose={() => (app.dialog = null)} oncreated={afterCreatedOrImported} />
{:else if app.dialog === "restore"}
  <RestoreDialog onclose={() => (app.dialog = null)} ondone={afterCreatedOrImported} />
{:else if app.dialog === "import"}
  <ImportDialog onclose={() => (app.dialog = null)} ondone={afterCreatedOrImported} />
{/if}

<style>
  .page {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .head {
    gap: 8px;
  }
  .name {
    font-size: 15px;
    font-weight: 600;
    text-align: left;
    white-space: normal;
  }
  .path {
    max-width: 260px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
  }
  .actions {
    gap: 4px;
    flex-wrap: nowrap;
  }
  .small {
    font-size: 12.5px;
  }
</style>
