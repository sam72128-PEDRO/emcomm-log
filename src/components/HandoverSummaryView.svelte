<script lang="ts">
  import { History, CircleAlert } from "@lucide/svelte";
  import { app } from "../lib/state.svelte";
  import { formatInstant } from "../lib/time";
  import { HANDLING_LABEL, KIND_LABEL } from "../lib/entryForm";
  import type * as T from "../lib/types";

  /** 2.3: read-only display of a handover summary (preview or reconstructed). */
  let { summary, onopen }: { summary: T.HandoverSummary; onopen: (entryId: string) => void } = $props();

  const when = (t: string | null) => (t ? formatInstant(t, app.zone, app.clock) : "—");
  const who = (o: T.OperatorSnapshot | null) =>
    o ? `${o.name}${o.callsign ? ` · ${o.callsign}` : ""}` : "—";
  const station = $derived(
    [summary.station_label, summary.tactical_id, summary.station_callsign].filter((x) => x).join(" · "),
  );
</script>

{#snippet refs(title: string, list: T.HandoverRefList, empty: string)}
  <section class="block">
    <h4>{title} <span class="badge neutral">{list.total}</span></h4>
    {#if list.total === 0}<p class="muted small">{empty}</p>{:else}
      <ul class="refs">
        {#each list.items as r (r.id)}
          <li>
            <button class="link mono" onclick={() => onopen(r.id)} aria-label="Open entry {r.sequence_no}"
              >#{String(r.sequence_no).padStart(4, "0")}</button
            >
            <span class="muted nowrap">{when(r.event_at_utc)}</span>
            <span class="muted">{KIND_LABEL[r.kind]}</span>
            <span class="summary">{r.summary || "(no summary)"}</span>
            {#if r.handling}<span class="badge info">{HANDLING_LABEL[r.handling]}</span>{/if}
            {#if r.important}<span class="badge warn">IMPORTANT</span>{/if}
            {#if r.flagged}<span class="badge danger">FLAGGED</span>{/if}
            {#if r.priority === "emergency"}<span class="badge danger">EMERGENCY</span
              >{:else if r.priority === "priority"}<span class="badge warn">PRIORITY</span>{/if}
            {#if r.follow_up_open}<span class="badge {r.overdue ? 'danger' : 'warn'}"
                >{r.overdue ? "FOLLOW-UP OVERDUE" : "FOLLOW-UP"}{r.follow_up_due_utc
                  ? ` · due ${when(r.follow_up_due_utc)}`
                  : ""}</span
              >{/if}
            {#if r.inherited}<span class="badge neutral" title="Logged before this shift began"
                >EARLIER SHIFT</span
              >{/if}
            {#if r.follow_up_open && r.follow_up_note}<div class="muted small fu">
                Follow-up: {r.follow_up_note}
              </div>{/if}
          </li>
        {/each}
      </ul>
      {#if list.total > list.items.length}<p class="muted small">
          … and {list.total - list.items.length} more. Use the Traffic Log filters to see all.
        </p>{/if}
    {/if}
  </section>
{/snippet}

<div class="handover">
  {#if summary.basis === "reconstructed"}
    <div class="banner warn" role="note">
      <History size={18} class="icon" />
      <div>
        <strong>Reconstructed from records</strong> on {when(summary.generated_at_utc)} — not an original snapshot.
        Entries are shown as they stand now, and outstanding work at the time of this handover is not recalculated.
      </div>
    </div>
  {:else}
    <p class="muted small" style="margin:0">
      Preview generated {when(summary.generated_at_utc)} from the current records. Nothing is saved or changed.
    </p>
  {/if}

  <dl class="kv">
    <dt>Logbook</dt>
    <dd>{summary.logbook_name}{summary.incident_name ? ` — ${summary.incident_name}` : ""}</dd>
    {#if station}<dt>Station</dt>
      <dd>{station}</dd>{/if}
    <dt>Outgoing</dt>
    <dd>{who(summary.outgoing)}</dd>
    <dt>Incoming</dt>
    <dd>{summary.incoming ? who(summary.incoming) : "Not selected"}</dd>
    <dt>Shift</dt>
    <dd>
      {summary.shift_start_utc ? when(summary.shift_start_utc) : "No shift recorded yet"} → {when(
        summary.handover_at_utc,
      )}
    </dd>
    {#if summary.period_name}<dt>Operational period</dt>
      <dd>{summary.period_name} ({when(summary.period_start_utc)} → {when(summary.period_end_utc)})</dd>{/if}
  </dl>

  <h3>This shift</h3>
  {#if summary.activity.total === 0}
    <p class="muted">No entries were logged during this shift.</p>
  {:else}
    <p>
      <strong>{summary.activity.total}</strong> entr{summary.activity.total === 1 ? "y" : "ies"} logged:
      {summary.activity.by_kind.map((k) => `${k.count} ${KIND_LABEL[k.kind]}`).join(", ")}
    </p>
  {/if}
  {#if summary.activity.voided || summary.activity.imported}<p class="muted small">
      {#if summary.activity.voided}{summary.activity.voided} voided.{/if}
      {#if summary.activity.imported}{summary.activity.imported} imported from ADIF (not counted above).{/if}
    </p>{/if}
  {#if summary.activity.significant.total > 0}
    {@render refs("Significant (Important, Flagged, Priority/Emergency)", summary.activity.significant, "")}
  {/if}

  {#if summary.outstanding}
    {@const o = summary.outstanding}
    <h3>Outstanding at the station <span class="muted small">(all shifts)</span></h3>
    {#if o.inherited_items > 0}<p class="muted small">
        <CircleAlert size={13} />
        {o.inherited_items} entr{o.inherited_items === 1 ? "y" : "ies"} below
        {o.inherited_items === 1 ? "was" : "were"} logged before this shift and inherited by it.
      </p>{/if}
    {@render refs("Not yet transmitted (Pending)", o.not_transmitted, "None.")}
    {@render refs("Transmitted, no acknowledgment recorded (Sent)", o.transmitted_no_ack, "None.")}
    {@render refs("Awaiting acknowledgment", o.awaiting_ack, "None.")}
    {@render refs(
      `Open follow-ups${o.overdue_follow_ups ? ` (${o.overdue_follow_ups} overdue)` : ""}`,
      o.open_follow_ups,
      "None.",
    )}
    {@render refs("Important entries", o.important, "None.")}
  {/if}

  {#if summary.shift_start_note || summary.handover_note}
    <h3>Notes</h3>
    <dl class="kv">
      {#if summary.shift_start_note}<dt>At start of this shift</dt>
        <dd>{summary.shift_start_note}</dd>{/if}
      {#if summary.handover_note}<dt>Handover note</dt>
        <dd>{summary.handover_note}</dd>{/if}
    </dl>
  {/if}
</div>

<style>
  .handover {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .handover h3 {
    margin: 8px 0 0;
    font-size: 15px;
  }
  .block h4 {
    margin: 4px 0;
    font-size: 13px;
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .refs {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .refs li {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: baseline;
  }
  .refs .summary {
    flex: 1 1 200px;
    min-width: 0;
  }
  .fu {
    flex-basis: 100%;
    padding-left: 52px;
  }
</style>
