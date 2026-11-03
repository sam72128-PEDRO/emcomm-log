<script lang="ts">
  import {
    CloudLightning,
    Radio,
    NotebookPen,
    FileText,
    ArrowLeftRight,
    Flag,
    SquarePen,
    List,
    Send,
    ClipboardList,
    HardDrive,
    ShieldCheck,
    ShieldAlert,
    FolderSearch,
    RefreshCw,
    PencilLine,
    Mail,
    Users,
    LoaderCircle,
  } from "@lucide/svelte";
  import { app, defaultQuery } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import { changeHandling, nextHandling } from "../lib/handling";
  import { backupNow } from "../lib/actions";
  import type * as T from "../lib/types";
  import { HANDLING_LABEL, TYPE_LABEL } from "../lib/entryForm";
  import { formatInstant, relativeAge } from "../lib/time";
  import EntryEditor from "../components/EntryEditor.svelte";
  import KindBadge from "../components/KindBadge.svelte";
  import Modal from "../components/Modal.svelte";
  import PeriodsPanel from "../components/PeriodsPanel.svelte";
  import { currentPeriod } from "../lib/periods";

  const info = $derived(app.active!.info);
  let recent = $state<T.Entry[]>([]);
  let pending = $state<T.Entry[]>([]);
  let notes = $state(app.active!.settings.operational_notes);
  let notesSaved = $state(app.active!.settings.operational_notes);
  let notesBusy = $state(false);
  let editIncident = $state(false);
  let incidentDraft = $state<T.IncidentFields>({ ...app.active!.info.incident });
  let nameDraft = $state(app.active!.info.name);
  let templates = $state<T.EntryTemplate[]>([]);

  async function load() {
    const [r, p] = await Promise.all([
      api
        .listEntries({ ...defaultQuery(), sort: "recorded", descending: true, limit: 5 })
        .catch(() => ({ items: [] as T.Entry[], total: 0 })),
      api
        .listEntries({
          ...defaultQuery(),
          handling: ["pending", "awaiting_ack", "sent"],
          descending: true,
          limit: 5,
        })
        .catch(() => ({ items: [] as T.Entry[], total: 0 })),
    ]);
    recent = r.items;
    pending = p.items;
  }
  $effect(() => {
    void app.dataVersion;
    load();
  });
  api
    .listTemplates()
    .then((t) => (templates = t))
    .catch(() => {});

  const HeaderIcon = $derived(
    info.logbook_type === "emergency" ? CloudLightning : info.logbook_type === "ham" ? Radio : NotebookPen,
  );
  const s = $derived(app.stats);
  const pendingCount = $derived(
    s ? (s.handling.pending ?? 0) + (s.handling.awaiting_ack ?? 0) + (s.handling.sent ?? 0) : 0,
  );
  const health = $derived(app.active!.health);
  const op = $derived(info.logbook_type === "emergency" ? currentPeriod(app.periods) : null);
  const b = $derived(app.backup);

  async function saveNotes() {
    notesBusy = true;
    try {
      const st = await api.updateLogbookSettings(app.active!.settings.time_display_override, notes);
      notesSaved = st.operational_notes;
      if (app.active) app.active.settings = st;
      app.toast("ok", "Operational notes saved to the logbook");
    } catch (e) {
      app.toast("error", `Notes not saved: ${toAppError(e).message}`);
    } finally {
      notesBusy = false;
    }
  }

  async function saveIncident() {
    try {
      const updated = await api.updateLogbookIdentity(nameDraft, incidentDraft);
      if (app.active) app.active.info = updated;
      editIncident = false;
      app.toast("ok", "Logbook details updated");
    } catch (e) {
      app.toast("error", toAppError(e).message);
    }
  }
</script>

<div class="dash">
  <section class="hero panel" aria-label="Active logbook">
    <div class="hero-main">
      <HeaderIcon size={54} strokeWidth={1.5} class="hero-icon" aria-hidden="true" />
      <div class="hero-text">
        <h1>{info.incident.incident_name || info.name}</h1>
        <p>
          {#if op}<span
              >Current period: {op.name} ({formatInstant(op.start_utc, app.zone, app.clock, {
                date: false,
              })}–{formatInstant(op.end_utc, app.zone, app.clock, { date: false })})</span
            >{:else if info.incident.operational_period}<span
              >Operational Period {info.incident.operational_period}</span
            >{/if}
          {#if info.incident.agency}<span>{info.incident.agency}</span>{/if}
          <span>Logbook: {info.name}</span>
          <span>{TYPE_LABEL[info.logbook_type]}</span>
        </p>
        <p class="sub">
          Operator: {app.profile?.name ?? "—"}{app.profile?.callsign ? ` (${app.profile.callsign})` : ""}{app
            .profile?.position
            ? ` · ${app.profile.position}`
            : ""}
        </p>
      </div>
      <button
        class="small ghost edit"
        onclick={() => {
          incidentDraft = { ...info.incident };
          nameDraft = info.name;
          editIncident = true;
        }}
        disabled={app.readOnly}><PencilLine size={14} />Edit details</button
      >
    </div>
    <div class="stats" aria-label="Logbook statistics">
      <div class="stat">
        <FileText size={24} aria-hidden="true" />
        <div><strong>{s?.today ?? "–"}</strong><span>Entries Today</span></div>
      </div>
      <div class="stat">
        <ArrowLeftRight size={24} aria-hidden="true" />
        <div><strong>{pendingCount}</strong><span>Pending Traffic</span></div>
      </div>
      <div class="stat">
        <Flag size={24} class="red" aria-hidden="true" />
        <div><strong>{s?.flagged ?? "–"}</strong><span>Flagged Reports</span></div>
      </div>
    </div>
  </section>

  <div class="strip">
    <div class="mini">
      <span class="muted">Total entries</span><strong>{s?.total_active ?? "–"}</strong><span class="muted"
        >{s?.total_void ?? 0} voided</span
      >
    </div>
    <div class="mini">
      <span class="muted"><Users size={14} /> Stations</span><strong>{s?.stations ?? "–"}</strong><span
        class="muted">distinct contacts</span
      >
    </div>
    <div class="mini">
      <span class="muted"><Mail size={14} /> Messages</span><strong
        >{s?.handling.pending ?? 0} / {s?.handling.awaiting_ack ?? 0} / {s?.handling.acknowledged ??
          0}</strong
      ><span class="muted">pending / awaiting ACK / ACK'd</span>
    </div>
    <div class="mini">
      <span class="muted">Last entry</span><strong
        >{s?.last_recorded_utc
          ? formatInstant(s.last_recorded_utc, app.zone, app.clock, { date: false })
          : "—"}</strong
      ><span class="muted">{relativeAge(s?.last_recorded_utc)}</span>
    </div>
  </div>

  <div class="grid-main">
    <section class="panel">
      <div class="panel-head">
        <h2><SquarePen size={20} class="icon" />Quick Entry</h2>
        <span class="spacer"></span><button class="link" onclick={() => app.navigate("quick")}
          >All fields →</button
        >
      </div>
      <div class="panel-body">
        <EntryEditor mode="new" compact {templates} />
      </div>
    </section>

    <section class="panel">
      <div class="panel-head">
        <h2><List size={20} class="icon" />Recent Entries</h2>
        <span class="spacer"></span><button class="link" onclick={() => app.navigate("log")}
          >View Logbook →</button
        >
      </div>
      <table class="data">
        <thead><tr><th>Time</th><th>Type</th><th>From / To</th><th>Summary</th><th>Status</th></tr></thead>
        <tbody>
          {#each recent as e (e.id)}
            <tr
              class="clickable"
              tabindex="0"
              onclick={() => (app.detailEntryId = e.id)}
              onkeydown={(ev) => ev.key === "Enter" && (app.detailEntryId = e.id)}
            >
              <td class="nowrap">{formatInstant(e.event_at_utc, app.zone, app.clock, { date: false })}</td>
              <td><KindBadge kind={e.kind} /></td>
              <td class="nowrap">{e.ham?.their_call || e.from_text || "—"} → {e.to_text || "—"}</td>
              <td class="ellipsis" style="max-width:300px">{e.subject}</td>
              <td
                >{#if e.handling && ["pending", "awaiting_ack", "sent"].includes(e.handling)}<span
                    class="badge warn">{HANDLING_LABEL[e.handling]}</span
                  >{:else}<span class="badge ok">Logged</span>{/if}</td
              >
            </tr>
          {:else}
            <tr><td colspan="5" class="empty">No entries yet.</td></tr>
          {/each}
        </tbody>
      </table>
    </section>
  </div>

  {#if info.logbook_type === "emergency"}<PeriodsPanel />{/if}

  <div class="grid-bottom">
    <section class="panel">
      <div class="panel-head">
        <h2><Send size={20} class="icon" />Pending Traffic</h2>
        <span class="spacer"></span><span class="badge neutral">{pendingCount} Items</span>
      </div>
      <ul class="pending">
        {#each pending as e (e.id)}
          <li>
            <button class="link title" onclick={() => (app.detailEntryId = e.id)}
              >{e.from_text || "—"} → {e.to_text || "—"}</button
            >
            <span class="muted ellipsis">{e.subject}</span>
            <button
              class="small"
              onclick={() => changeHandling(e, nextHandling(e.handling))}
              disabled={app.readOnly || !!app.storageBlocked}
              title="Advance handling status (recorded as a revision)"
            >
              {HANDLING_LABEL[e.handling!]} → {HANDLING_LABEL[nextHandling(e.handling)]}
            </button>
          </li>
        {:else}
          <li class="empty">No traffic awaiting handling.</li>
        {/each}
      </ul>
      {#if pendingCount > pending.length}<div class="panel-body">
          <button class="link" onclick={() => app.navigate("messages")}>All messages →</button>
        </div>{/if}
    </section>

    <section class="panel">
      <div class="panel-head"><h2><HardDrive size={20} class="icon" />Storage &amp; Backup</h2></div>
      <div class="panel-body col">
        <div class="row">
          {#if health.ok && !app.storageBlocked}<span class="badge ok"
              ><ShieldCheck size={13} />LIVE STORAGE OK</span
            >{:else}<span class="badge danger"><ShieldAlert size={13} />STORAGE PROBLEM</span>{/if}
          <span
            class="muted small"
            title="Journal mode and synchronous level verified when the logbook opened"
            >{health.journal_mode.toUpperCase()} · sync {health.synchronous === 2
              ? "FULL"
              : health.synchronous}</span
          >
          <button
            class="small ghost icon-only"
            aria-label="Recheck storage"
            title="Recheck storage"
            onclick={() => app.recheckStorage()}><RefreshCw size={14} /></button
          >
        </div>
        <div class="mono small path" title={info.path}>{info.path}</div>
        {#if app.storageBlocked}<div class="field-error">{app.storageBlocked.message}</div>{/if}
        <div class="row" style="margin-top:6px">
          {#if b?.running}<span class="badge info"><LoaderCircle size={13} class="spin" />BACKUP RUNNING</span
            >
          {:else if !b?.configured}<span class="badge warn">BACKUP NOT CONFIGURED</span>
          {:else if b?.last && !b.last.ok}<span class="badge danger">BACKUP FAILED</span>
          {:else if b?.last_success}<span class="badge ok">BACKUP SUCCEEDED</span>
          {:else}<span class="badge neutral">NO BACKUP YET</span>{/if}
        </div>
        {#if b?.last && !b.last.ok}<div class="field-error small">
            Last attempt {relativeAge(b.last.finished_at_utc)}: {b.last.message}
          </div>{/if}
        {#if b?.last_success}<div class="small muted">
            Last verified backup {formatInstant(b.last_success.finished_at_utc, app.zone, app.clock)} ({b
              .last_success.kind}, {b.last_success.entry_count} entries)
          </div>{/if}
        {#if b?.message}<div class="small muted">{b.message}</div>{/if}
        {#if b?.configured}<div class="small muted">
            Automatic every {b.interval_minutes} min while open, and on close.
          </div>{/if}
        <div class="row wrap">
          <button class="small" onclick={backupNow} disabled={!b?.configured || b?.running}
            >Back Up Now</button
          >
          {#if !b?.configured}<button class="small" onclick={() => app.navigate("settings")}
              >Configure…</button
            >{/if}
          <button
            class="small"
            onclick={() => api.revealPath(info.path).catch((e) => app.toast("error", toAppError(e).message))}
            ><FolderSearch size={14} />Reveal</button
          >
          <button class="small" onclick={() => app.navigate("reports")}>Export…</button>
        </div>
      </div>
    </section>

    <section class="panel">
      <div class="panel-head"><h2><ClipboardList size={20} class="icon" />Operational Notes</h2></div>
      <div class="panel-body col">
        <label for="dash-notes" class="sr-only">Operational notes for this logbook</label>
        <textarea
          id="dash-notes"
          rows="6"
          bind:value={notes}
          maxlength="10000"
          disabled={app.readOnly}
          placeholder="Net schedule, primary/backup frequencies, conditions…"></textarea>
        <div class="row">
          <span class="hint">{notes === notesSaved ? "Saved in this logbook" : "Unsaved changes"}</span>
          <span class="spacer"></span>
          <button
            class="small primary"
            onclick={saveNotes}
            disabled={notes === notesSaved || notesBusy || app.readOnly}>Save notes</button
          >
        </div>
      </div>
    </section>
  </div>
</div>

{#if editIncident}
  <Modal title="Logbook details" onclose={() => (editIncident = false)}>
    <div class="field">
      <label for="di-name">Logbook name</label><input
        id="di-name"
        type="text"
        bind:value={nameDraft}
        maxlength="120"
      />
    </div>
    <div class="grid-2">
      <div class="field">
        <label for="di-inc"
          >Incident name{#if info.logbook_type === "emergency"}<span class="req">*</span>{/if}</label
        ><input id="di-inc" type="text" bind:value={incidentDraft.incident_name} />
      </div>
      <div class="field">
        <label for="di-num">Incident number</label><input
          id="di-num"
          type="text"
          bind:value={incidentDraft.incident_number}
        />
      </div>
      <div class="field">
        <label for="di-op">Operational period (text)</label><input
          id="di-op"
          type="text"
          bind:value={incidentDraft.operational_period}
        />
      </div>
      <div class="field">
        <label for="di-ag">Agency / organization</label><input
          id="di-ag"
          type="text"
          bind:value={incidentDraft.agency}
        />
      </div>
      <div class="field" style="grid-column:span 2">
        <label for="di-loc">Location</label><input
          id="di-loc"
          type="text"
          bind:value={incidentDraft.location}
        />
      </div>
    </div>
    <p class="hint" style="margin:0">
      New entries snapshot the current incident and operational period; existing entries keep theirs.
      {#if info.logbook_type === "emergency"}Structured periods with start and end times (used by ICS forms)
        are managed in the Operational Periods panel.{/if}
    </p>
    {#snippet footer()}
      <button onclick={() => (editIncident = false)}>Cancel</button>
      <button class="primary" onclick={saveIncident}>Save</button>
    {/snippet}
  </Modal>
{/if}

<style>
  .dash {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .hero {
    display: flex;
    gap: 18px;
    align-items: center;
    padding: 16px 20px;
    background:
      radial-gradient(ellipse at 80% 0%, color-mix(in srgb, var(--cyan) 14%, transparent), transparent 60%),
      linear-gradient(120deg, var(--panel-2), var(--panel));
  }
  .hero-main {
    flex: 1;
    display: flex;
    gap: 18px;
    align-items: center;
    min-width: 0;
    position: relative;
  }
  :global(.hero-icon) {
    color: var(--text);
    flex: none;
    padding-right: 16px;
    border-right: 2px solid var(--ok);
    box-sizing: content-box;
  }
  .hero-text {
    min-width: 0;
  }
  .hero-text h1 {
    font-size: 28px;
  }
  .hero-text p {
    margin: 6px 0 0;
    display: flex;
    flex-wrap: wrap;
    gap: 0;
    color: var(--text-2);
  }
  .hero-text p span + span::before {
    content: "|";
    margin: 0 10px;
    color: var(--muted);
  }
  .hero-text .sub {
    font-size: 13px;
    color: var(--muted);
  }
  .edit {
    position: absolute;
    top: -6px;
    right: 0;
  }
  .stats {
    display: flex;
    background: color-mix(in srgb, var(--bg) 60%, transparent);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 10px 6px;
  }
  .stat {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 0 18px;
    color: var(--cyan);
  }
  .stat + .stat {
    border-left: 1px solid var(--border);
  }
  .stat strong {
    display: block;
    font-size: 28px;
    color: var(--text);
    line-height: 1.1;
  }
  .stat span {
    color: var(--text-2);
    font-size: 13px;
  }
  :global(.red) {
    color: var(--danger);
  }
  .strip {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 12px;
  }
  .mini {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 10px 14px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 10px;
  }
  .mini strong {
    font-size: 20px;
  }
  .mini .muted {
    font-size: 12.5px;
    display: flex;
    gap: 5px;
    align-items: center;
  }
  .grid-main {
    display: grid;
    grid-template-columns: minmax(380px, 2fr) 3fr;
    gap: 16px;
    align-items: start;
  }
  .grid-bottom {
    display: grid;
    grid-template-columns: 1.3fr 1fr 1fr;
    gap: 16px;
    align-items: start;
  }
  .pending {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .pending li {
    display: grid;
    grid-template-columns: auto 1fr auto;
    gap: 10px;
    align-items: center;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border);
  }
  .pending .title {
    font-weight: 600;
    color: var(--text);
  }
  .small {
    font-size: 12.5px;
  }
  .path {
    word-break: break-all;
  }
  @media (max-width: 1300px) {
    .grid-main,
    .grid-bottom {
      grid-template-columns: 1fr;
    }
    .hero {
      flex-direction: column;
      align-items: stretch;
    }
  }
</style>
