<script lang="ts">
  import {
    Star,
    PencilLine,
    Ban,
    Undo2,
    Copy,
    Download,
    Paperclip,
    History,
    CircleAlert,
    FileText,
  } from "@lucide/svelte";
  import Modal from "./Modal.svelte";
  import EntryEditor from "./EntryEditor.svelte";
  import KindBadge from "./KindBadge.svelte";
  import Form213Dialog from "./Form213Dialog.svelte";
  import { periodById } from "../lib/periods";
  import { app } from "../lib/state.svelte";
  import { api, newId, toAppError } from "../lib/api";
  import { confirm } from "../lib/confirm.svelte";
  import type * as T from "../lib/types";
  import { DIRECTION_LABEL, HANDLING_LABEL, PRIORITY_LABEL } from "../lib/entryForm";
  import { formatInstant, offsetLabel } from "../lib/time";

  let entry = $state<T.Entry | null>(null);
  let revisions = $state<T.EntryRevision[]>([]);
  let error = $state<string | null>(null);
  let editing = $state(false);
  let editDraft = $state<T.Draft | null>(null);
  let busy = $state(false);
  let form213 = $state(false);

  async function load() {
    const id = app.detailEntryId;
    if (!id) return;
    try {
      [entry, revisions] = await Promise.all([api.getEntry(id), api.entryRevisions(id)]);
      error = null;
    } catch (e) {
      error = toAppError(e).message;
    }
  }
  $effect(() => {
    void app.detailEntryId;
    load().then(() => {
      if (app.editDraft && entry && app.editDraft.entry_id === entry.id) {
        editDraft = app.editDraft;
        app.editDraft = null;
        editing = true;
      }
    });
  });

  function close() {
    app.detailEntryId = null;
  }

  const fmt = (iso: string | null | undefined) => formatInstant(iso, app.zone, app.clock, { seconds: true });

  async function changeStatus(toVoid: boolean) {
    if (!entry) return;
    const a = await confirm.ask({
      title: toVoid ? `Void entry #${entry.sequence_no}?` : `Restore entry #${entry.sequence_no}?`,
      message: toVoid
        ? "Voiding hides the entry from the default log and reports. It is not deleted: its number is never reused and you can restore it."
        : "The entry becomes active again. This is recorded in its revision history.",
      input: { label: "Reason (optional)", value: "", maxLength: 500 },
      buttons: [
        { id: "ok", label: toVoid ? "Void entry" : "Restore entry", tone: toVoid ? "danger" : "primary" },
        { id: "cancel", label: "Cancel" },
      ],
      cancelId: "cancel",
    });
    if (a.id !== "ok") return;
    busy = true;
    try {
      const r = toVoid
        ? await api.voidEntry(entry.id, a.value, newId())
        : await api.restoreEntry(entry.id, a.value, newId());
      app.saveStatus = {
        kind: "saved",
        seq: r.entry.sequence_no,
        at: r.entry.updated_at_utc,
        edit: true,
        duplicate: false,
      };
      app.toast(
        "ok",
        toVoid
          ? `Entry #${r.entry.sequence_no} voided (revision ${r.entry.current_revision})`
          : `Entry #${r.entry.sequence_no} restored`,
      );
      app.bumpData();
      await load();
    } catch (e) {
      app.noteSaveFailure(toAppError(e));
      app.toast("error", toAppError(e).message);
    } finally {
      busy = false;
    }
  }

  async function toggleFav() {
    if (!entry) return;
    try {
      entry = await api.setFavorite(entry.id, !entry.favorite);
      app.bumpData();
    } catch (e) {
      app.toast("error", toAppError(e).message);
    }
  }

  async function exportThis() {
    if (!entry) return;
    app.logQuery = { ...app.logQuery, ids: [entry.id], status: "all" };
    app.reportScope = "filter";
    close();
    await app.navigate("reports");
  }

  async function copyAsNew() {
    if (!entry) return;
    app.editorRequest = { mode: "copy", entry };
    close();
    await app.navigate("quick");
  }

  // Flatten nested snapshots so the timeline can list exactly which fields changed.
  function flat(o: unknown, prefix = "", out: Record<string, string> = {}): Record<string, string> {
    if (o && typeof o === "object" && !Array.isArray(o)) {
      for (const [k, v] of Object.entries(o)) {
        if (["updated_at_utc", "current_revision", "operator_snapshot"].includes(k)) continue;
        flat(v, prefix ? `${prefix}.${k}` : k, out);
      }
    } else if (Array.isArray(o)) {
      out[prefix] = o
        .map((x) =>
          typeof x === "object" && x && "display_name" in x
            ? (x as { display_name: string }).display_name
            : String(x),
        )
        .join(", ");
    } else {
      out[prefix] = o === null || o === undefined ? "" : String(o);
    }
    return out;
  }
  function diff(r: T.EntryRevision): { field: string; from: string; to: string }[] {
    if (!r.before) return [];
    const a = flat(r.before);
    const b = flat(r.after);
    const keys = [...new Set([...Object.keys(a), ...Object.keys(b)])];
    return keys
      .filter((k) => (a[k] ?? "") !== (b[k] ?? ""))
      .map((k) => ({ field: k.replace(/_utc$/, "").replace(/_/g, " "), from: a[k] ?? "", to: b[k] ?? "" }));
  }
  const isTime = (f: string, v: string) => /at|start|end|time/.test(f) && /^\d{4}-\d{2}-\d{2}T/.test(v);
  const show = (f: string, v: string) =>
    v === "" ? "∅" : isTime(f, v) ? fmt(v) : v.length > 160 ? v.slice(0, 160) + "…" : v;

  const emFields: [keyof T.EmergencyFields, string][] = [
    ["incident_name", "Incident"],
    ["incident_number", "Incident number"],
    ["operational_period", "Operational period"],
    ["message_number", "Message number"],
    ["precedence", "Precedence"],
    ["originator", "Originator"],
    ["originator_position", "Originator position"],
    ["recipient", "Recipient"],
    ["recipient_position", "Recipient position"],
    ["sender", "Sent by"],
    ["receiver", "Received by"],
    ["sent_at_utc", "Sent time"],
    ["received_at_utc", "Received time"],
    ["action_requested", "Requested action / reply"],
    ["response", "Response"],
    ["resource_unit", "Resource / unit"],
    ["assignment", "Assignment"],
    ["activity_start_utc", "Activity start"],
    ["activity_end_utc", "Activity end"],
    ["location", "Location"],
    ["disposition", "Disposition"],
    ["preparer", "Prepared by"],
    ["approved_by_name", "Approved by"],
    ["approved_by_position", "Approved by (position)"],
    ["replied_by_name", "Replied by"],
    ["replied_by_position", "Replied by (position)"],
    ["replied_at_utc", "Replied at"],
  ];
</script>

{#if editing && entry}
  <Modal
    title="Edit entry #{entry.sequence_no}"
    size="xl"
    onclose={() => app.flushDrafts().then(() => (editing = false))}
  >
    <EntryEditor
      mode="edit"
      {entry}
      initialDraft={editDraft}
      onsaved={async () => {
        editing = false;
        editDraft = null;
        await load();
      }}
      oncancel={() => {
        editing = false;
        editDraft = null;
      }}
    />
  </Modal>
{:else}
  <Modal
    title={entry ? `Entry #${String(entry.sequence_no).padStart(4, "0")}` : "Entry"}
    size="xl"
    onclose={close}
    {busy}
  >
    {#if error}<div class="banner error" role="alert"><CircleAlert size={18} class="icon" />{error}</div>{/if}
    {#if entry}
      <div class="row wrap">
        <KindBadge kind={entry.kind} />
        <span class="badge neutral">{DIRECTION_LABEL[entry.direction]}</span>
        <span
          class="badge {entry.priority === 'routine'
            ? 'neutral'
            : entry.priority === 'emergency'
              ? 'danger'
              : 'warn'}">{PRIORITY_LABEL[entry.priority]}</span
        >
        {#if entry.status === "void"}<span class="badge danger">VOIDED {fmt(entry.voided_at_utc)}</span
          >{:else}<span class="badge ok">ACTIVE</span>{/if}
        {#if entry.handling}<span class="badge info">{HANDLING_LABEL[entry.handling]}</span>{/if}
        {#if entry.flagged}<span class="badge danger">FLAGGED FOR REPORT</span>{/if}
        <span class="badge neutral">Revision {entry.current_revision}</span>
      </div>
      {#if entry.status === "void" && entry.void_reason}<p class="muted" style="margin:0">
          Void reason: {entry.void_reason}
        </p>{/if}
      <h2 class="subject">{entry.subject}</h2>
      <dl class="kv">
        <dt>Event time</dt>
        <dd>
          {fmt(entry.event_at_utc)}
          <span class="muted"
            >(entered as {entry.event_timezone}, {offsetLabel(entry.event_utc_offset_min)})</span
          >
        </dd>
        <dt>Recorded</dt>
        <dd>
          {fmt(entry.recorded_at_utc)}{#if entry.updated_at_utc !== entry.recorded_at_utc}
            · last changed {fmt(entry.updated_at_utc)}{/if}
        </dd>
        <dt>From → To</dt>
        <dd>{entry.from_text || "—"} → {entry.to_text || "—"}</dd>
        {#if entry.op_id}<dt>Operational period</dt>
          <dd>
            {periodById(app.periods, entry.op_id)?.name ?? "(unknown period)"}{periodById(
              app.periods,
              entry.op_id,
            )?.status === "void"
              ? " (voided)"
              : ""}
          </dd>{/if}
        {#if entry.channel}<dt>Channel</dt>
          <dd>{entry.channel}</dd>{/if}
        {#if entry.location}<dt>Location</dt>
          <dd>{entry.location}</dd>{/if}
        {#if entry.tags.length}<dt>Tags</dt>
          <dd>{entry.tags.join(", ")}</dd>{/if}
        <dt>Operator</dt>
        <dd>
          {entry.operator_snapshot.name}{entry.operator_snapshot.callsign
            ? ` (${entry.operator_snapshot.callsign})`
            : ""}
        </dd>
      </dl>
      {#if entry.body}<pre class="body">{entry.body}</pre>{/if}
      {#if entry.ham}
        <fieldset>
          <legend>QSO</legend>
          <dl class="kv">
            <dt>Call</dt>
            <dd class="mono">{entry.ham.their_call}</dd>
            <dt>Start / end</dt>
            <dd>
              {fmt(entry.ham.qso_start_utc)}{#if entry.ham.qso_end_utc}
                → {fmt(entry.ham.qso_end_utc)}{/if}
            </dd>
            <dt>Frequency / band</dt>
            <dd>{entry.ham.frequency_hz ? `${entry.ham.frequency_hz / 1e6} MHz` : "—"} {entry.ham.band}</dd>
            <dt>Mode</dt>
            <dd>{entry.ham.mode} {entry.ham.submode}</dd>
            <dt>RST sent / rcvd</dt>
            <dd>{entry.ham.rst_sent || "—"} / {entry.ham.rst_rcvd || "—"}</dd>
            <dt>Grids</dt>
            <dd>{entry.ham.their_grid || "—"} (theirs) · {entry.ham.my_grid || "—"} (mine)</dd>
            <dt>Station / operator</dt>
            <dd>{entry.ham.station_call || "—"} / {entry.ham.operator_call || "—"}</dd>
            {#if entry.ham.qso_notes}<dt>Notes</dt>
              <dd>{entry.ham.qso_notes}</dd>{/if}
          </dl>
        </fieldset>
      {/if}
      {#if entry.emergency}
        <fieldset>
          <legend
            >{entry.emergency.record_type === "message"
              ? "Message details"
              : entry.emergency.record_type === "activity"
                ? "Activity details"
                : "Incident"}</legend
          >
          <dl class="kv">
            {#each emFields as [k, l] (k)}
              {@const v = entry.emergency[k]}
              {#if v !== "" && v !== null && v !== false}<dt>{l}</dt>
                <dd>{typeof v === "string" && k.endsWith("_utc") ? fmt(v) : String(v)}</dd>{/if}
            {/each}
            {#if entry.emergency.reply_requested}<dt>Reply requested</dt>
              <dd>Yes</dd>{/if}
          </dl>
        </fieldset>
      {/if}
      {#if entry.attachments.length}
        <fieldset>
          <legend>Attachments</legend>
          <ul class="atts">
            {#each entry.attachments as a (a.id)}
              <li>
                <button
                  class="link"
                  onclick={() =>
                    api.openAttachment(a.id).catch((e) => app.toast("error", toAppError(e).message))}
                  ><Paperclip size={13} />{a.display_name}</button
                >
                <span class="muted"
                  >{Math.ceil(a.size_bytes / 1024)} KB · SHA-256 {a.sha256.slice(0, 12)}…</span
                >
              </li>
            {/each}
          </ul>
        </fieldset>
      {/if}
      <fieldset>
        <legend><History size={14} /> Revision history</legend>
        <ol class="timeline">
          {#each revisions as r (r.revision)}
            <li>
              <div class="row wrap">
                <strong
                  >Rev {r.revision} · {r.action === "create"
                    ? "Created"
                    : r.action === "edit"
                      ? "Edited"
                      : r.action === "void"
                        ? "Voided"
                        : "Restored"}</strong
                ><span class="muted"
                  >{fmt(r.at_utc)} by {r.editor.name}{r.editor.callsign
                    ? ` (${r.editor.callsign})`
                    : ""}</span
                >
              </div>
              {#if r.reason}<div class="muted">Reason: {r.reason}</div>{/if}
              {#each diff(r) as d (d.field)}
                <div class="change">
                  <span class="f">{d.field}:</span> <del>{show(d.field, d.from)}</del> →
                  <ins>{show(d.field, d.to)}</ins>
                </div>
              {/each}
            </li>
          {/each}
        </ol>
      </fieldset>
    {/if}
    {#snippet footer()}
      {#if entry}
        <button onclick={toggleFav} aria-pressed={entry.favorite} disabled={app.readOnly}
          ><Star size={16} fill={entry.favorite ? "currentColor" : "none"} />{entry.favorite
            ? "Unfavorite"
            : "Favorite"}</button
        >
        <button onclick={copyAsNew} disabled={app.readOnly}><Copy size={16} />Use as new</button>
        <button onclick={exportThis}><Download size={16} />Export Selection</button>
        {#if entry.kind === "message" && entry.status === "active" && app.bookType === "emergency"}<button
            onclick={() => (form213 = true)}><FileText size={16} />ICS 213…</button
          >{/if}
        <span class="spacer"></span>
        {#if entry.status === "active"}
          <button
            class="danger"
            onclick={() => changeStatus(true)}
            disabled={busy || app.readOnly || !!app.storageBlocked}><Ban size={16} />Void</button
          >
          <button
            class="primary"
            onclick={() => (editing = true)}
            disabled={app.readOnly || !!app.storageBlocked}><PencilLine size={16} />Edit</button
          >
        {:else}
          <button
            class="primary"
            onclick={() => changeStatus(false)}
            disabled={busy || app.readOnly || !!app.storageBlocked}><Undo2 size={16} />Restore</button
          >
        {/if}
      {/if}
    {/snippet}
  </Modal>
{/if}

{#if form213 && entry}<Form213Dialog entryIds={[entry.id]} onclose={() => (form213 = false)} />{/if}

<style>
  .subject {
    font-size: 19px;
  }
  .kv {
    display: grid;
    grid-template-columns: 170px 1fr;
    gap: 6px 14px;
    margin: 0;
  }
  .kv dt {
    color: var(--muted);
  }
  .kv dd {
    margin: 0;
    word-break: break-word;
  }
  .body {
    white-space: pre-wrap;
    word-break: break-word;
    font-family: inherit;
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 12px;
    margin: 0;
    max-height: 320px;
    overflow: auto;
  }
  .atts {
    margin: 0;
    padding-left: 18px;
  }
  .timeline {
    margin: 0;
    padding-left: 20px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .change {
    font-size: 13px;
    word-break: break-word;
  }
  .change .f {
    color: var(--muted);
  }
  del {
    color: var(--danger);
  }
  ins {
    color: var(--ok);
    text-decoration: none;
  }
  legend :global(svg) {
    vertical-align: -2px;
  }
</style>
