<script lang="ts">
  import { IS_ANDROID } from "../lib/platform";
  import { onMount } from "svelte";
  import { FolderOpen, CircleAlert, ShieldCheck } from "@lucide/svelte";
  import Modal from "./Modal.svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import type { BackupListing, BackupPreview } from "../lib/types";
  import { TYPE_LABEL } from "../lib/entryForm";
  import { formatInstant } from "../lib/time";

  let { onclose, ondone }: { onclose: () => void; ondone: () => void } = $props();

  let listing = $state<BackupListing[]>([]);
  let listErr = $state<string | null>(null);
  let source = $state(app.settings?.backup.destination ?? "");
  let selected = $state<string | null>(null);
  let preview = $state<BackupPreview | null>(null);
  let previewErr = $state<string | null>(null);
  let parent = $state(app.settings?.default_logbook_parent ?? "");
  let name = $state("");
  let busy = $state(false);
  let phase = $state("");

  async function loadList() {
    listErr = null;
    listing = [];
    if (!source) return;
    try {
      listing = await api.listBackups(source);
    } catch (e) {
      listErr = toAppError(e).message;
    }
  }
  onMount(loadList);

  async function chooseSource() {
    const p = await api.pickFolder(
      "Choose a backup folder (destination root or a single backup)",
      source || null,
    );
    if (!p) return;
    source = p;
    await loadList();
    if (listing.length === 0) await select(p);
  }

  async function select(path: string) {
    selected = path;
    preview = null;
    previewErr = null;
    busy = true;
    phase = "Verifying backup (hashes and database integrity)…";
    try {
      preview = await api.previewBackup(path);
      name = `${preview.name} (restored ${new Date().toISOString().slice(0, 10)})`;
    } catch (e) {
      previewErr = toAppError(e).message;
    } finally {
      busy = false;
      phase = "";
    }
  }

  async function browseParent() {
    const p = await api.pickFolder("Restore into which folder?", parent || null);
    if (p) parent = p;
  }

  async function restore() {
    if (!selected || !preview || !parent) return;
    busy = true;
    phase = "Restoring as a new logbook and verifying…";
    try {
      const row = await api.restoreBackup(selected, parent, name.trim() || null);
      app.toast(
        "ok",
        `Restored "${row.name}" with ${row.entry_count} entries. The original logbook was not changed.`,
      );
      ondone();
      onclose();
    } catch (e) {
      previewErr = toAppError(e).message;
      app.announce(`Restore failed: ${previewErr}`, true);
    } finally {
      busy = false;
      phase = "";
    }
  }
</script>

<Modal title="Restore backup as a new logbook" size="xl" {onclose} {busy}>
  <p class="muted" style="margin:0">
    Restoring never overwrites an existing logbook. The backup is verified, then copied into a new, separate
    logbook folder.
  </p>
  <div class="field">
    <label for="rs-source">Backup location</label>
    <div class="row">
      <input id="rs-source" type="text" readonly value={source} placeholder="Choose a folder" />
      <button onclick={chooseSource}><FolderOpen size={16} />{IS_ANDROID ? "Choose…" : "Browse…"}</button>
    </div>
  </div>
  {#if listErr}<div class="banner error"><CircleAlert size={18} class="icon" />{listErr}</div>{/if}
  {#if listing.length}
    <div class="list">
      <table class="data">
        <thead
          ><tr><th>Logbook</th><th>Type</th><th>Created</th><th>Kind</th><th>Entries</th><th></th></tr></thead
        >
        <tbody>
          {#each listing as b (b.path)}
            <tr aria-selected={selected === b.path}>
              <td>{b.name}</td>
              <td>{TYPE_LABEL[b.logbook_type]}</td>
              <td class="nowrap">{formatInstant(b.created_at_utc, app.zone, app.clock)}</td>
              <td>{b.kind}</td>
              <td>{b.entry_count}</td>
              <td><button class="small" onclick={() => select(b.path)} disabled={busy}>Select</button></td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else if source && !listErr}
    <p class="muted">No completed backups found in this folder.</p>
  {/if}
  {#if phase}<p class="muted" aria-live="polite">{phase}</p>{/if}
  {#if previewErr}<div class="banner error" role="alert">
      <CircleAlert size={18} class="icon" />
      <div>
        <strong>This backup cannot be restored.</strong>
        {previewErr}<br />No logbook was created or changed.
      </div>
    </div>{/if}
  {#if preview}
    <div class="banner ok">
      <ShieldCheck size={18} class="icon" />
      <div>
        <strong>Verified:</strong>
        {preview.name} ({TYPE_LABEL[preview.logbook_type]}) — {preview.entry_count} entries, {preview.attachment_count}
        attachment(s), backed up {formatInstant(preview.created_at_utc, app.zone, app.clock)}.
        {#if preview.first_event_utc}Events {formatInstant(
            preview.first_event_utc,
            app.zone,
            app.clock,
          ).slice(0, 10)} to {formatInstant(preview.last_event_utc, app.zone, app.clock).slice(0, 10)}.{/if}
      </div>
    </div>
    <div class="grid-2">
      <div class="field">
        <label for="rs-name">Name for the restored logbook</label>
        <input id="rs-name" type="text" bind:value={name} maxlength="120" />
      </div>
      <div class="field">
        <label for="rs-parent">Restore into folder</label>
        <div class="row">
          <input id="rs-parent" type="text" readonly value={parent} />
          {#if !IS_ANDROID}<button onclick={browseParent}><FolderOpen size={16} />Browse…</button>{/if}
        </div>
      </div>
    </div>
  {/if}
  {#snippet footer()}
    <button onclick={onclose} disabled={busy}>Cancel</button>
    <button class="primary" onclick={restore} disabled={busy || !preview || !parent}
      >Restore as new logbook</button
    >
  {/snippet}
</Modal>

<style>
  .list {
    max-height: 260px;
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: 8px;
  }
</style>
