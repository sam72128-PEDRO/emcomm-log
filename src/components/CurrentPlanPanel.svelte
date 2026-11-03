<script lang="ts">
  import { RadioTower, PencilLine } from "@lucide/svelte";
  import { app } from "../lib/state.svelte";
  import { currentPeriod } from "../lib/periods";
  import ChannelPlanDialog from "./ChannelPlanDialog.svelte";

  let editing = $state(false);
  let now = $state(Date.now());
  $effect(() => {
    const t = setInterval(() => (now = Date.now()), 60_000);
    return () => clearInterval(t);
  });
  const op = $derived(currentPeriod(app.periods, now));
  $effect(() => {
    if (op) void app.loadPlan(op.id);
  });
  const plan = $derived(op ? app.plans[op.id] : undefined);
  const mode = (m: string) => ({ A: "Analog", D: "Digital", M: "Mixed" })[m as "A" | "D" | "M"] ?? "";
  const freq = (f: string, nw: string, tone: string) =>
    f ? [f, nw, tone ? `(${tone})` : ""].filter(Boolean).join(" ") : "—";
</script>

{#if op}
  <section class="panel">
    <div class="panel-head">
      <h2><RadioTower size={20} class="icon" />Channel Plan — {op.name}</h2>
      <span class="spacer"></span>
      <button class="small" onclick={() => (editing = true)} disabled={app.readOnly}
        ><PencilLine size={14} />{plan?.rows.length ? "Edit plan" : "Enter ICS 205…"}</button
      >
    </div>
    {#if plan?.rows.length}
      <table class="data">
        <thead
          ><tr
            ><th>Ch</th><th>Function</th><th>Channel</th><th>Assignment</th><th>RX MHz</th><th>TX MHz</th><th
              >Mode</th
            ><th>Remarks</th></tr
          ></thead
        >
        <tbody>
          {#each plan.rows as r, i (i)}
            <tr>
              <td class="nowrap">{[r.zone_group, r.channel_number].filter(Boolean).join(" / ") || "—"}</td>
              <td>{r.function || "—"}</td>
              <td><strong>{r.channel_name || "—"}</strong></td>
              <td>{r.assignment || "—"}</td>
              <td class="nowrap mono">{freq(r.rx_freq, r.rx_nw, r.rx_tone)}</td>
              <td class="nowrap mono">{freq(r.tx_freq, r.tx_nw, r.tx_tone)}</td>
              <td>{mode(r.mode) || "—"}</td>
              <td class="ellipsis" style="max-width:220px">{r.remarks}</td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if plan.special_instructions}<div class="panel-body">
          <span class="muted small">Special instructions:</span>
          {plan.special_instructions}
        </div>{/if}
    {:else}
      <div class="panel-body">
        <p class="muted" style="margin:0">
          No channel plan for this period. Enter the ICS 205 you were given and its channels are offered in
          every entry's Channel field.
        </p>
      </div>
    {/if}
  </section>
  {#if editing}<ChannelPlanDialog period={op} onclose={() => (editing = false)} />{/if}
{/if}

<style>
  .small {
    font-size: 12.5px;
  }
</style>
