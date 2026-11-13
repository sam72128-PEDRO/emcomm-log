<script lang="ts">
  import { Mail } from "@lucide/svelte";
  import { app, defaultQuery } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import type * as T from "../lib/types";
  import { HANDLING_LABEL, PRIORITY_LABEL } from "../lib/entryForm";
  import { changeHandling, outstandingAction, OUTSTANDING_HANDLING } from "../lib/handling";
  import { formatInstant } from "../lib/time";
  import KindBadge from "../components/KindBadge.svelte";

  type Tab = "open" | "pending" | "sent" | "awaiting_ack" | "acknowledged" | "other" | "all";
  let tab = $state<Tab>("open");
  let items = $state<T.Entry[]>([]);
  let total = $state(0);
  let error = $state<string | null>(null);
  let busyId = $state<string | null>(null);
  // 2.3: latest recorded status change per row (time and operator), read from revision history.
  let latest = $state<Record<string, T.LatestHandlingChange>>({});

  // Transmitted (Sent) and Awaiting ACK are separate: transmitting a message never means it was acknowledged.
  const tabs: { id: Tab; label: string; states: T.Handling[] }[] = [
    { id: "open", label: "Needs action", states: OUTSTANDING_HANDLING },
    { id: "pending", label: "Not transmitted", states: ["pending"] },
    { id: "sent", label: "Transmitted, no ACK recorded", states: ["sent"] },
    { id: "awaiting_ack", label: "Awaiting ACK", states: ["awaiting_ack"] },
    { id: "acknowledged", label: "Acknowledged", states: ["acknowledged"] },
    { id: "other", label: "Relayed / Delivered / Closed", states: ["relayed", "delivered", "closed"] },
    {
      id: "all",
      label: "All tracked",
      states: ["pending", "sent", "awaiting_ack", "acknowledged", "relayed", "delivered", "closed"],
    },
  ];
  const states = $derived(tabs.find((t) => t.id === tab)!.states);

  async function load() {
    try {
      const p = await api.listEntries({ ...defaultQuery(), handling: states, limit: 200 });
      items = p.items;
      total = p.total;
      error = null;
      try {
        const rows = items.length ? await api.latestHandlingChanges(items.map((e) => e.id)) : [];
        latest = Object.fromEntries((Array.isArray(rows) ? rows : []).map((r) => [r.entry_id, r]));
      } catch {
        latest = {};
      }
    } catch (e) {
      error = toAppError(e).message;
    }
  }
  $effect(() => {
    void app.dataVersion;
    void tab;
    load();
  });

  const count = (t: (typeof tabs)[number]) => t.states.reduce((n, s) => n + (app.stats?.handling[s] ?? 0), 0);

  async function change(e: T.Entry, el: HTMLSelectElement) {
    busyId = e.id;
    const ok = await changeHandling(e, (el.value || null) as T.Handling | null);
    // A declined correction changes nothing: put the select back on the recorded status (the bound value did
    // not change, so Svelte would not rewrite it).
    if (!ok) el.value = e.handling ?? "";
    busyId = null;
  }
</script>

<section class="panel">
  <div class="panel-head">
    <h1 class="title"><Mail size={22} class="icon" />Messages</h1>
    <span class="spacer"></span>
    <span class="muted"
      >Entries with a handling status. Changing a status saves a new revision; nothing is acknowledged
      automatically.</span
    >
  </div>
  <div class="tabs" role="tablist" aria-label="Message status">
    {#each tabs as t (t.id)}
      <button role="tab" aria-selected={tab === t.id} onclick={() => (tab = t.id)}
        >{t.label} <span class="badge neutral">{count(t)}</span></button
      >
    {/each}
  </div>
  <div class="panel-body">
    {#if error}<div class="banner error" role="alert">{error}</div>{/if}
    <div class="table-wrap">
      <table class="data">
        <thead
          ><tr
            ><th>#</th><th>Time</th><th>Type</th><th>From → To</th><th>Summary</th><th>Priority</th><th
              >Handling</th
            ><th>Outstanding action</th><th>Last status change</th></tr
          ></thead
        >
        <tbody>
          {#each items as e (e.id)}
            <tr>
              <td class="mono"
                ><button
                  class="link"
                  onclick={() => (app.detailEntryId = e.id)}
                  aria-label="Open entry {e.sequence_no}">{String(e.sequence_no).padStart(4, "0")}</button
                ></td
              >
              <td class="when">{formatInstant(e.event_at_utc, app.zone, app.clock)}</td>
              <td><KindBadge kind={e.kind} /></td>
              <td>{e.from_text || "—"} → {e.to_text || "—"}</td>
              <td class="ellipsis" style="max-width:220px"
                >{e.subject}{#if e.emergency?.message_number}&nbsp;
                  <span class="muted">(Msg {e.emergency.message_number})</span>{/if}</td
              >
              <td>{PRIORITY_LABEL[e.priority]}</td>
              <td>
                <select
                  class="handling-select"
                  aria-label="Handling status for entry {e.sequence_no}"
                  value={e.handling ?? ""}
                  onchange={(ev) => change(e, ev.currentTarget)}
                  disabled={busyId === e.id || app.readOnly || !!app.storageBlocked}
                >
                  {#each Object.entries(HANDLING_LABEL) as [v, l] (v)}<option value={v}>{l}</option>{/each}
                  <option value="">Not tracked</option>
                </select>
              </td>
              <td class="small">
                {outstandingAction(e.handling) || "—"}
                {#if e.follow_up?.state === "open"}<div>
                    <span
                      class="badge warn"
                      title="A separate follow-up; it does not change the message status">FOLLOW-UP OPEN</span
                    >
                  </div>{/if}
                {#if e.related_count}<div class="muted">{e.related_count} related</div>{/if}
              </td>
              <td class="small when">
                {#if latest[e.id]}{formatInstant(latest[e.id].at_utc, app.zone, app.clock)}
                  <div class="muted">
                    {latest[e.id].by_name || "—"}{latest[e.id].by_callsign
                      ? ` · ${latest[e.id].by_callsign}`
                      : ""}
                  </div>{:else}—{/if}
              </td>
            </tr>
          {:else}
            <tr
              ><td colspan="9" class="empty"
                >No messages in this state. Set "Message handling" when logging traffic to track it here.</td
              ></tr
            >
          {/each}
        </tbody>
      </table>
    </div>
    {#if total > items.length}<p class="muted">
        Showing the first {items.length} of {total}. Use the Traffic Log handling filter to see all.
      </p>{/if}
  </div>
</section>

<style>
  .when {
    min-width: 8.5em;
  }
  .table-wrap {
    overflow-x: auto;
  }
  .tabs {
    flex-wrap: wrap;
  }
  .handling-select {
    min-width: 9.5em;
  }
  .title {
    display: flex;
    gap: 10px;
    align-items: center;
    font-size: 20px;
  }
</style>
