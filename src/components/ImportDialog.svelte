<script lang="ts">
  import { IS_ANDROID, DEVICE, shownPath } from "../lib/platform";
  import { FolderOpen, CircleAlert, ShieldCheck, PackageOpen } from "@lucide/svelte";
  import Modal from "./Modal.svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import type { PackagePreview } from "../lib/types";
  import { TYPE_LABEL } from "../lib/entryForm";
  import { formatInstant } from "../lib/time";

  let { onclose, ondone }: { onclose: () => void; ondone: () => void } = $props();

  let file = $state("");
  let preview = $state<PackagePreview | null>(null);
  let err = $state<string | null>(null);
  let parent = $state(app.settings?.default_logbook_parent ?? "");
  let name = $state("");
  let busy = $state(false);
  let phase = $state("");

  async function choose() {
    const p = await api.pickFile("Choose an EMCOMM LOG package", [
      { name: "EMCOMM LOG package", extensions: ["emlog"] },
    ]);
    if (!p) return;
    file = p;
    preview = null;
    err = null;
    busy = true;
    phase = "Validating package…";
    try {
      preview = await api.previewPackage(p);
      // Default to a name that will not collide with the original if it is in the same folder.
      name = `${preview.name} (imported ${new Date().toISOString().slice(0, 10)})`.slice(0, 120);
    } catch (e) {
      err = toAppError(e).message;
    } finally {
      busy = false;
      phase = "";
    }
  }

  async function browseParent() {
    const p = await api.pickFolder("Import into which folder?", parent || null);
    if (p) parent = p;
  }

  async function doImport() {
    if (!preview || !parent) return;
    busy = true;
    phase = "Importing and verifying…";
    err = null;
    try {
      const out = await api.importPackage(file, parent, name.trim() || null);
      const dup = out.result.duplicate_of.length
        ? ` Note: this package came from a logbook already on this ${DEVICE} (${out.result.duplicate_of.join(", ")}); it was imported as a separate copy.`
        : "";
      app.toast(
        dup ? "warn" : "ok",
        `Imported "${out.logbook.name}" with ${out.result.entry_count} entries.${dup}`,
        12000,
      );
      ondone();
      onclose();
    } catch (e) {
      err = toAppError(e).message;
      app.announce(`Import failed: ${err}`, true);
    } finally {
      busy = false;
      phase = "";
    }
  }
</script>

<Modal title="Import .emlog package" size="lg" {onclose} {busy}>
  <p class="muted" style="margin:0">
    Import always creates a new local logbook. Nothing on this {DEVICE} is overwritten or merged. Operator profiles
    are not imported; the logbook's own settings, templates, favorites, revisions and attachments are.
  </p>
  <div class="field">
    <label for="im-file">Package file</label>
    <div class="row">
      <input id="im-file" type="text" readonly value={shownPath(file)} placeholder="Choose a .emlog file" />
      <button onclick={choose} disabled={busy} data-autofocus><PackageOpen size={16} />Choose…</button>
    </div>
  </div>
  {#if phase}<p class="muted" aria-live="polite">{phase}</p>{/if}
  {#if err}<div class="banner error" role="alert">
      <CircleAlert size={18} class="icon" />
      <div>
        <strong>{preview ? "Not imported." : "Package rejected."}</strong>
        {err}<br />No logbook was created.
      </div>
    </div>{/if}
  {#if preview}
    <div class="banner ok">
      <ShieldCheck size={18} class="icon" />
      <div>
        <strong>Valid package:</strong>
        {preview.name} ({TYPE_LABEL[preview.logbook_type]}), {preview.entry_count} entries, {preview.attachment_count}
        attachment(s), exported {formatInstant(preview.created_at_utc, app.zone, app.clock)} by EMCOMM LOG {preview.app_version}.
      </div>
    </div>
    <div class="grid-2">
      <div class="field">
        <label for="im-name">Name for the imported logbook</label>
        <input id="im-name" type="text" bind:value={name} maxlength="120" />
      </div>
      <div class="field">
        <label for="im-parent">Import into folder</label>
        <div class="row">
          <input id="im-parent" type="text" readonly value={parent} />
          {#if !IS_ANDROID}<button onclick={browseParent}><FolderOpen size={16} />Browse…</button>{/if}
        </div>
      </div>
    </div>
  {/if}
  {#snippet footer()}
    <button onclick={onclose} disabled={busy}>Cancel</button>
    <button class="primary" onclick={doImport} disabled={busy || !preview || !parent}
      >Import as new logbook</button
    >
  {/snippet}
</Modal>
