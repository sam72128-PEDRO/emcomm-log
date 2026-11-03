<script lang="ts">
  import { FileClock, TriangleAlert } from "@lucide/svelte";
  import Modal from "./Modal.svelte";
  import { app } from "../lib/state.svelte";
  import { api } from "../lib/api";
  import { confirm } from "../lib/confirm.svelte";
  import { restoreDraft } from "../lib/recovery";
  import type * as T from "../lib/types";
  import { formatInstant, relativeAge } from "../lib/time";

  const summary = (d: T.Draft) => (d.content as { form?: { subject?: string; body?: string } }).form ?? {};

  function close() {
    app.pendingRecovery = [];
    app.dialog = null;
  }

  function keep(d: T.Draft) {
    app.pendingRecovery = app.pendingRecovery.filter((x) => x.draft_id !== d.draft_id);
    app.announce("Draft kept for later. Find it in Settings → Storage & Recovery.");
    if (!app.pendingRecovery.length) close();
  }

  async function discard(d: T.Draft) {
    const a = await confirm.ask({
      title: "Discard draft?",
      message:
        "This unfinished entry will be permanently removed from the recovery area. It was never part of the logbook.",
      buttons: [
        { id: "discard", label: "Discard", tone: "danger" },
        { id: "cancel", label: "Cancel", tone: "primary" },
      ],
      cancelId: "cancel",
    });
    if (a.id !== "discard") return;
    await api.deleteDraft(d.logbook_id, d.draft_id).catch(() => {});
    keep(d);
  }

  async function restore(d: T.Draft) {
    const others = app.pendingRecovery.filter((x) => x.draft_id !== d.draft_id);
    close();
    const ok = await restoreDraft(d);
    if (!ok) {
      app.pendingRecovery = [d, ...others];
      app.dialog = "recovery";
    } else if (others.length) {
      app.toast(
        "info",
        `${others.length} other draft(s) kept for later in Settings → Storage & Recovery.`,
        9000,
      );
    }
  }
</script>

<Modal title="Recover unfinished entries" size="lg" onclose={close}>
  <div class="banner info">
    <FileClock size={18} class="icon" />
    <div>
      These entries were being typed when {app.active?.info.name} was last closed unexpectedly or left unsaved.
      They are <strong>drafts</strong>, not logbook entries. Restoring puts the text back in the editor; it is
      saved only when you choose Save Entry or Save Changes.
    </div>
  </div>
  <ul class="drafts">
    {#each app.pendingRecovery as d (d.draft_id)}
      {@const f = summary(d)}
      <li>
        <div class="grow">
          <strong
            >{d.editor === "edit"
              ? `Changes to entry #${d.entry_sequence}`
              : "New entry (Quick Entry)"}</strong
          >
          <span class="muted"
            >· last saved {formatInstant(d.saved_at_utc, app.zone, app.clock, { seconds: true })} ({relativeAge(
              d.saved_at_utc,
            )})</span
          >
          <div class="preview">
            {f.subject || "(no summary yet)"}{#if f.body}
              — <span class="muted">{f.body.slice(0, 140)}{f.body.length > 140 ? "…" : ""}</span>{/if}
          </div>
          {#if !d.intact}<div class="field-error">
              <TriangleAlert size={13} /> This draft failed its checksum and may be incomplete. You can still discard
              it or keep it for later.
            </div>{/if}
        </div>
        <div class="row">
          <button class="primary small" onclick={() => restore(d)} disabled={!d.intact}>Restore</button>
          <button class="small" onclick={() => keep(d)}>Keep for Later</button>
          <button class="small danger" onclick={() => discard(d)}>Discard</button>
        </div>
      </li>
    {/each}
  </ul>
  {#snippet footer()}<button onclick={close}>Keep all for later</button>{/snippet}
</Modal>

<style>
  .drafts {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .drafts li {
    display: flex;
    gap: 12px;
    align-items: center;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  .preview {
    margin-top: 3px;
  }
</style>
