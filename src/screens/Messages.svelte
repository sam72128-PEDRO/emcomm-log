<script lang="ts">
  import { Mail } from "@lucide/svelte";
  import { app, defaultQuery } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import type * as T from "../lib/types";
  import { HANDLING_LABEL } from "../lib/entryForm";
  import { changeHandling } from "../lib/handling";
  import { formatInstant } from "../lib/time";
  import KindBadge from "../components/KindBadge.svelte";

  type Tab = "open" | T.Handling | "all";
  let tab = $state<Tab>("open");
  let items = $state<T.Entry[]>([]);
  let total = $state(0);
  let error = $state<string | null>(null);
  let busyId = $state<string | null>(null);

  const tabs: { id: Tab; label: string; states: T.Handling[] }[] = [
    { id: "open", label: "Needs action", states: ["pending", "sent", "awaiting_ack"] },
    { id: "pending", label: "Pending (to send)", states: ["pending"] },
    { id: "awaiting_ack", label: "Sent, awaiting ACK", states: ["sent", "awaiting_ack"] },
    { id: "acknowledged", label: "Acknowledged", states: ["acknowledged"] },
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

  async function change(e: T.Entry, v: string) {
    busyId = e.id;
    await changeHandling(e, (v || null) as T.Handling | null);
    busyId = null;
  }
</script>

<section class="panel">
  <div class="panel-head">
    <h1 class="title"><Mail size={22} class="icon" />Messages</h1>
    <span class="spacer"></span>
    <span class="muted">Entries with a handling status. Changing a status saves a new revision.</span>
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
    <table class="data">
      <thead
        ><tr
          ><th>#</th><th>Time</th><th>Type</th><th>From → To</th><th>Summary</th><th>Priority</th><th
            >Handling</th
          ></tr
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
            <td class="nowrap">{formatInstant(e.event_at_utc, app.zone, app.clock)}</td>
            <td><KindBadge kind={e.kind} /></td>
            <td class="nowrap">{e.from_text || "—"} → {e.to_text || "—"}</td>
            <td class="ellipsis" style="max-width:380px"
              >{e.subject}{#if e.emergency?.message_number}
                <span class="muted">(Msg {e.emergency.message_number})</span>{/if}</td
            >
            <td>{e.priority}</td>
            <td>
              <select
                aria-label="Handling status for entry {e.sequence_no}"
                value={e.handling ?? ""}
                onchange={(ev) => change(e, ev.currentTarget.value)}
                disabled={busyId === e.id || app.readOnly || !!app.storageBlocked}
              >
                {#each Object.entries(HANDLING_LABEL) as [v, l] (v)}<option value={v}>{l}</option>{/each}
                <option value="">Not tracked</option>
              </select>
            </td>
          </tr>
        {:else}
          <tr
            ><td colspan="7" class="empty"
              >No messages in this state. Set "Message handling" when logging traffic to track it here.</td
            ></tr
          >
        {/each}
      </tbody>
    </table>
    {#if total > items.length}<p class="muted">
        Showing the first {items.length} of {total}. Use the Traffic Log handling filter to see all.
      </p>{/if}
  </div>
</section>

<style>
  .title {
    display: flex;
    gap: 10px;
    align-items: center;
    font-size: 20px;
  }
</style>
