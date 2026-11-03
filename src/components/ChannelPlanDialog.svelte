<script lang="ts">
  import { Plus, Trash2, ArrowUp, ArrowDown, Copy, CircleAlert } from "@lucide/svelte";
  import { tick, untrack } from "svelte";
  import Modal from "./Modal.svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import { confirm } from "../lib/confirm.svelte";
  import { blankChannel } from "../lib/plan";
  import type * as T from "../lib/types";

  let { period, onclose }: { period: T.OperationalPeriod; onclose: () => void } = $props();
  const op = untrack(() => period);

  let rows = $state<T.ChannelRow[]>([]);
  let instructions = $state("");
  let loaded = $state(false);
  let busy = $state(false);
  let error = $state<{ field: string; message: string } | null>(null);
  // Copy sources: "op:<id>" (this logbook) or "book:<logbookId>:<opId>" (another logbook).
  let source = $state("");
  let otherBooks = $state<{ id: string; name: string; sources: T.PlanSource[] }[]>([]);
  const others = $derived(app.periods.filter((p) => p.id !== op.id && p.status === "active"));

  type Col = { key: keyof T.ChannelRow; label: string; width: string; kind?: "nw" | "mode" };
  const COLS: Col[] = [
    { key: "zone_group", label: "Zone/Group", width: "70px" },
    { key: "channel_number", label: "Ch #", width: "44px" },
    { key: "function", label: "Function", width: "90px" },
    { key: "channel_name", label: "Channel name / talkgroup", width: "120px" },
    { key: "assignment", label: "Assignment", width: "110px" },
    { key: "rx_freq", label: "RX MHz", width: "80px" },
    { key: "rx_nw", label: "RX N/W", width: "52px", kind: "nw" },
    { key: "rx_tone", label: "RX tone/NAC", width: "64px" },
    { key: "tx_freq", label: "TX MHz", width: "80px" },
    { key: "tx_nw", label: "TX N/W", width: "52px", kind: "nw" },
    { key: "tx_tone", label: "TX tone/NAC", width: "64px" },
    { key: "mode", label: "Mode", width: "60px", kind: "mode" },
    { key: "remarks", label: "Remarks", width: "140px" },
  ];

  api
    .periodPlan(op.id)
    .then((p) => {
      rows = p.rows.length ? p.rows : [blankChannel()];
      instructions = p.special_instructions;
    })
    .catch((e) => app.toast("error", toAppError(e).message))
    .finally(async () => {
      loaded = true;
      await tick();
      document.getElementById("ch-0-zone_group")?.focus();
    });

  // Other emergency logbooks on this computer that have a plan to copy (read-only).
  api
    .listLogbooks()
    .then(async (books) => {
      const candidates = books.filter(
        (b) => b.id !== app.active?.info.id && b.logbook_type === "emergency" && b.available,
      );
      const found = [];
      for (const b of candidates) {
        const sources = await api.planSources(b.id).catch(() => []);
        if (sources.length) found.push({ id: b.id, name: b.name, sources });
      }
      otherBooks = found;
    })
    .catch(() => {});

  async function add(after = rows.length - 1) {
    rows.splice(after + 1, 0, blankChannel());
    await tick();
    document.getElementById(`ch-${after + 1}-zone_group`)?.focus();
  }

  function move(i: number, d: number) {
    const j = i + d;
    if (j < 0 || j >= rows.length) return;
    [rows[i], rows[j]] = [rows[j], rows[i]];
  }

  function keydown(ev: KeyboardEvent, i: number) {
    if (ev.key === "Enter" && !ev.metaKey && !ev.ctrlKey) {
      ev.preventDefault();
      add(i);
    }
  }

  async function copy() {
    if (!source) return;
    const hasContent = rows.some((r) => Object.values(r).some((v) => v)) || instructions.trim();
    if (hasContent) {
      const a = await confirm.ask({
        title: "Replace this plan?",
        message: "The channels and instructions in the editor will be replaced by the copied plan.",
        detail: "Nothing is saved until you choose Save plan.",
        buttons: [
          { id: "cancel", label: "Cancel", tone: "primary" },
          { id: "ok", label: "Replace" },
        ],
        cancelId: "cancel",
      });
      if (a.id !== "ok") return;
    }
    try {
      const [kind, a, b] = source.split(":");
      const p = kind === "op" ? await api.periodPlan(a) : await api.readPlanFrom(a, b);
      rows = p.rows.length ? p.rows.map((r) => ({ ...r })) : [blankChannel()];
      instructions = p.special_instructions;
      app.announce(`Copied ${p.rows.length} channel(s); review and save`);
    } catch (e) {
      app.toast("error", toAppError(e).message);
    }
  }

  async function save() {
    busy = true;
    error = null;
    try {
      const saved = await api.setPeriodPlan(op.id, { rows, special_instructions: instructions });
      app.plans = { ...app.plans, [op.id]: saved };
      app.toast("ok", `Channel plan saved for “${op.name}” (${saved.rows.length} channels)`);
      onclose();
    } catch (e) {
      const err = toAppError(e);
      error = { field: err.field ?? "", message: err.message };
      app.announce(`Not saved: ${err.message}`, true);
      const m = /^plan\.(\d+)\.(\w+)$/.exec(err.field ?? "");
      if (m) document.getElementById(`ch-${m[1]}-${m[2]}`)?.focus();
    } finally {
      busy = false;
    }
  }

  const invalid = (i: number, key: string) => error?.field === `plan.${i}.${key}`;
</script>

<Modal title={`Channel plan — ${op.name}`} {onclose} size="xl" {busy}>
  <p class="hint" style="margin:0">
    Type in the ICS 205 you were given, one row per channel. Press Enter in a row to add the next. Choosing a
    channel in an entry's Channel field fills its name and receive frequency.
  </p>
  {#if error}<div class="banner error" role="alert">
      <CircleAlert size={18} class="icon" />
      <div><strong>Not saved.</strong> {error.message}</div>
    </div>{/if}
  {#if loaded}
    <div class="scroll">
      <table class="data plan">
        <thead>
          <tr>
            <th>#</th>
            {#each COLS as c (c.key)}<th style="min-width:{c.width}">{c.label}</th>{/each}
            <th><span class="sr-only">Actions</span></th>
          </tr>
        </thead>
        <tbody>
          {#each rows as r, i (i)}
            <tr>
              <td class="muted">{i + 1}</td>
              {#each COLS as c (c.key)}
                <td>
                  {#if c.kind === "nw"}
                    <select
                      id="ch-{i}-{c.key}"
                      aria-label="{c.label}, row {i + 1}"
                      bind:value={r[c.key]}
                      aria-invalid={invalid(i, c.key) ? "true" : undefined}
                      ><option value=""></option><option value="N">N</option><option value="W">W</option
                      ></select
                    >
                  {:else if c.kind === "mode"}
                    <select
                      id="ch-{i}-{c.key}"
                      aria-label="{c.label}, row {i + 1}"
                      bind:value={r[c.key]}
                      aria-invalid={invalid(i, c.key) ? "true" : undefined}
                      ><option value=""></option><option value="A">A</option><option value="D">D</option
                      ><option value="M">M</option></select
                    >
                  {:else}
                    <input
                      id="ch-{i}-{c.key}"
                      type="text"
                      aria-label="{c.label}, row {i + 1}"
                      bind:value={r[c.key]}
                      inputmode={c.key.endsWith("freq") ? "decimal" : undefined}
                      aria-invalid={invalid(i, c.key) ? "true" : undefined}
                      onkeydown={(e) => keydown(e, i)}
                    />
                  {/if}
                </td>
              {/each}
              <td class="nowrap">
                <button
                  class="small ghost icon-only"
                  aria-label="Move row {i + 1} up"
                  title="Move up"
                  onclick={() => move(i, -1)}
                  disabled={i === 0}><ArrowUp size={14} /></button
                >
                <button
                  class="small ghost icon-only"
                  aria-label="Move row {i + 1} down"
                  title="Move down"
                  onclick={() => move(i, 1)}
                  disabled={i === rows.length - 1}><ArrowDown size={14} /></button
                >
                <button
                  class="small ghost icon-only"
                  aria-label="Remove row {i + 1}"
                  title="Remove"
                  onclick={() => (rows = rows.length > 1 ? rows.filter((_, k) => k !== i) : [blankChannel()])}
                  ><Trash2 size={14} /></button
                >
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <div class="row wrap">
      <button class="small" onclick={() => add()}><Plus size={14} />Add channel</button>
      <span class="spacer"></span>
      {#if others.length || otherBooks.length}
        <label for="ch-copy" class="small">Copy from</label>
        <select id="ch-copy" bind:value={source} style="max-width:280px">
          <option value="">Choose a plan…</option>
          {#if others.length}<optgroup label="This logbook"
              >{#each others as p (p.id)}<option value="op:{p.id}">{p.name}</option>{/each}</optgroup
            >{/if}
          {#each otherBooks as b (b.id)}
            <optgroup label={b.name}
              >{#each b.sources as s (s.op_id)}<option value="book:{b.id}:{s.op_id}"
                  >{s.op_name} ({s.rows} channels)</option
                >{/each}</optgroup
            >
          {/each}
        </select>
        <button class="small" onclick={copy} disabled={!source}><Copy size={14} />Copy</button>
      {/if}
    </div>
    <div class="field">
      <label for="ch-instr">Special instructions (ICS 205 block 5)</label>
      <textarea id="ch-instr" rows="3" bind:value={instructions} maxlength="10000"></textarea>
    </div>
  {:else}<p class="muted">Loading…</p>{/if}
  {#snippet footer()}
    <button onclick={onclose}>Cancel</button>
    <button class="primary" onclick={save} disabled={busy || !loaded}>Save plan</button>
  {/snippet}
</Modal>

<style>
  .scroll {
    overflow-x: auto;
    max-width: 100%;
  }
  .plan td {
    padding: 3px 4px;
  }
  .plan input,
  .plan select {
    width: 100%;
    min-width: 0;
    padding: 4px 6px;
    font-size: 12.5px;
  }
  .plan th {
    font-size: 11.5px;
    white-space: nowrap;
  }
</style>
