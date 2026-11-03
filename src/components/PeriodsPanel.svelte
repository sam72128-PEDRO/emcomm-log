<script lang="ts">
  import {
    CalendarClock,
    RadioTower,
    Plus,
    PencilLine,
    Users,
    FileText,
    Ban,
    RotateCcw,
  } from "@lucide/svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import { confirm } from "../lib/confirm.svelte";
  import { currentPeriod } from "../lib/periods";
  import { formatInstant } from "../lib/time";
  import type * as T from "../lib/types";
  import PeriodDialog from "./PeriodDialog.svelte";
  import ResourcesDialog from "./ResourcesDialog.svelte";
  import Form214Dialog from "./Form214Dialog.svelte";
  import Form309Dialog from "./Form309Dialog.svelte";
  import ChannelPlanDialog from "./ChannelPlanDialog.svelte";

  let showVoided = $state(false);
  let editing = $state<{ period: T.OperationalPeriod | null; start: string | null } | null>(null);
  let resourcesFor = $state<T.OperationalPeriod | null>(null);
  let form214For = $state<string | null | undefined>(undefined);
  let form309For = $state<string | null | undefined>(undefined);
  let planFor = $state<T.OperationalPeriod | null>(null);
  // Re-evaluated each minute so "current" follows the clock.
  let now = $state(Date.now());
  $effect(() => {
    const t = setInterval(() => (now = Date.now()), 60_000);
    return () => clearInterval(t);
  });

  const current = $derived(currentPeriod(app.periods, now));
  const shown = $derived(app.periods.filter((p) => showVoided || p.status === "active"));
  const voidedCount = $derived(app.periods.filter((p) => p.status === "void").length);
  const latest = $derived(
    [...app.periods]
      .filter((p) => p.status === "active")
      .sort((a, b) => Date.parse(b.end_utc) - Date.parse(a.end_utc))[0],
  );

  const fmt = (iso: string) => formatInstant(iso, app.zone, app.clock);

  async function setVoid(p: T.OperationalPeriod, v: boolean) {
    if (v) {
      const r = await confirm.ask({
        title: `Void “${p.name}”?`,
        message:
          "The period is hidden from pickers and can't take new entries. Entries already in it keep it.",
        detail: "You can restore it later.",
        buttons: [
          { id: "cancel", label: "Cancel", tone: "primary" },
          { id: "void", label: "Void period", tone: "danger" },
        ],
        cancelId: "cancel",
      });
      if (r.id !== "void") return;
    }
    try {
      await api.setPeriodVoid(p.id, v);
      await app.refreshPeriods();
      app.toast("ok", `“${p.name}” ${v ? "voided" : "restored"}`);
    } catch (e) {
      app.toast("error", toAppError(e).message);
    }
  }
</script>

<section class="panel">
  <div class="panel-head">
    <h2><CalendarClock size={20} class="icon" />Operational Periods</h2>
    <span class="spacer"></span>
    <button
      class="small"
      onclick={() => (form214For = null)}
      disabled={!app.periods.some((p) => p.status === "active")}><FileText size={14} />ICS 214…</button
    >
    <button
      class="small"
      onclick={() => (form309For = null)}
      disabled={!app.periods.some((p) => p.status === "active")}><FileText size={14} />ICS 309…</button
    >
    <button
      class="small primary"
      onclick={() => (editing = { period: null, start: latest?.end_utc ?? null })}
      disabled={app.readOnly}><Plus size={14} />New period</button
    >
  </div>
  {#if shown.length}
    <ul class="ops">
      {#each shown as p (p.id)}
        <li class:void={p.status === "void"}>
          <div class="grow">
            <strong>{p.name}</strong>
            {#if p.status === "void"}<span class="badge neutral">VOIDED</span>
            {:else if current?.id === p.id}<span class="badge ok">CURRENT</span>{/if}
            <div class="muted small">{fmt(p.start_utc)} → {fmt(p.end_utc)}</div>
            {#if p.notes}<div class="muted small ellipsis">{p.notes}</div>{/if}
          </div>
          {#if p.status === "active"}
            <button class="small ghost" onclick={() => (planFor = p)} title="Channel plan (from the ICS 205)"
              ><RadioTower size={14} />Channels</button
            >
            <button
              class="small ghost"
              onclick={() => (resourcesFor = p)}
              disabled={app.readOnly}
              title="Resources assigned (ICS 214 block 6)"><Users size={14} />Resources</button
            >
            <button class="small ghost" onclick={() => (form214For = p.id)} title="ICS 214 for this period"
              ><FileText size={14} />214</button
            >
            <button class="small ghost" onclick={() => (form309For = p.id)} title="ICS 309 for this period"
              ><FileText size={14} />309</button
            >
            <button
              class="small ghost icon-only"
              aria-label="Edit {p.name}"
              title="Edit"
              onclick={() => (editing = { period: p, start: null })}
              disabled={app.readOnly}><PencilLine size={14} /></button
            >
            <button
              class="small ghost icon-only"
              aria-label="Void {p.name}"
              title="Void"
              onclick={() => setVoid(p, true)}
              disabled={app.readOnly}><Ban size={14} /></button
            >
          {:else}
            <button class="small ghost" onclick={() => setVoid(p, false)} disabled={app.readOnly}
              ><RotateCcw size={14} />Restore</button
            >
          {/if}
        </li>
      {/each}
    </ul>
  {:else}
    <div class="panel-body">
      <p class="muted" style="margin:0">
        No operational periods yet. Add one so new entries are grouped by period and you can produce an ICS
        214.
      </p>
    </div>
  {/if}
  {#if voidedCount}
    <div class="panel-body">
      <label class="check small"
        ><input type="checkbox" bind:checked={showVoided} /> Show {voidedCount} voided</label
      >
    </div>
  {/if}
</section>

{#if editing}<PeriodDialog
    period={editing.period}
    suggestedStart={editing.start}
    onclose={() => (editing = null)}
  />{/if}
{#if resourcesFor}<ResourcesDialog period={resourcesFor} onclose={() => (resourcesFor = null)} />{/if}
{#if form214For !== undefined}<Form214Dialog
    opId={form214For}
    onclose={() => (form214For = undefined)}
  />{/if}
{#if planFor}<ChannelPlanDialog period={planFor} onclose={() => (planFor = null)} />{/if}
{#if form309For !== undefined}<Form309Dialog
    opId={form309For}
    onclose={() => (form309For = undefined)}
  />{/if}

<style>
  .ops {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .ops li {
    display: flex;
    gap: 6px;
    align-items: center;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border);
  }
  .ops li.void {
    opacity: 0.6;
  }
  .ops strong {
    margin-right: 6px;
  }
  .grow {
    flex: 1;
    min-width: 0;
  }
  .small {
    font-size: 12.5px;
  }
</style>
