<script lang="ts">
  import { Plus, Trash2, ArrowUp, ArrowDown, Copy } from "@lucide/svelte";
  import { tick, untrack } from "svelte";
  import Modal from "./Modal.svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import type * as T from "../lib/types";

  let { period, onclose }: { period: T.OperationalPeriod; onclose: () => void } = $props();

  let rows = $state<T.OpResource[]>([]);
  let loaded = $state(false);
  let busy = $state(false);
  let copyFrom = $state("");
  const others = $derived(app.periods.filter((p) => p.id !== period.id));

  api
    .periodResources(untrack(() => period.id))
    .then((r) => (rows = r.length ? r : [blank()]))
    .catch((e) => app.toast("error", toAppError(e).message))
    .finally(async () => {
      loaded = true;
      // Rows arrive after the dialog opened; put the cursor in the first cell.
      await tick();
      document.getElementById("res-name-0")?.focus();
    });

  function blank(): T.OpResource {
    return { name: "", ics_position: "", home_agency: "" };
  }

  async function add(after = rows.length - 1) {
    rows.splice(after + 1, 0, blank());
    await tick();
    document.getElementById(`res-name-${after + 1}`)?.focus();
  }

  function move(i: number, d: number) {
    const j = i + d;
    if (j < 0 || j >= rows.length) return;
    [rows[i], rows[j]] = [rows[j], rows[i]];
  }

  async function copy() {
    if (!copyFrom) return;
    try {
      const src = await api.periodResources(copyFrom);
      rows = src.length ? src.map((r) => ({ ...r })) : [blank()];
      app.announce(`Copied ${src.length} resource(s); review and save`);
    } catch (e) {
      app.toast("error", toAppError(e).message);
    }
  }

  function keydown(ev: KeyboardEvent, i: number) {
    if (ev.key === "Enter" && !ev.metaKey && !ev.ctrlKey) {
      ev.preventDefault();
      add(i);
    }
  }

  async function save() {
    busy = true;
    try {
      const saved = await api.setPeriodResources(period.id, rows);
      app.toast("ok", `${saved.length} resource(s) saved for “${period.name}”`);
      onclose();
    } catch (e) {
      app.toast("error", `Not saved: ${toAppError(e).message}`);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title={`Resources assigned — ${period.name}`} {onclose} size="lg" {busy}>
  <p class="hint" style="margin:0">
    ICS 214 block 6. Press Enter in a row to add the next one. The first 8 fit on the form; more continue on
    an attached page.
  </p>
  {#if loaded}
    <table class="data res">
      <thead
        ><tr
          ><th>#</th><th>Name</th><th>ICS position</th><th>Home agency (and unit)</th><th
            ><span class="sr-only">Actions</span></th
          ></tr
        ></thead
      >
      <tbody>
        {#each rows as r, i (i)}
          <tr>
            <td class="muted">{i + 1}</td>
            <td
              ><input
                id="res-name-{i}"
                type="text"
                aria-label="Name, row {i + 1}"
                bind:value={r.name}
                maxlength="120"
                onkeydown={(e) => keydown(e, i)}
              /></td
            >
            <td
              ><input
                type="text"
                aria-label="ICS position, row {i + 1}"
                bind:value={r.ics_position}
                maxlength="120"
                onkeydown={(e) => keydown(e, i)}
              /></td
            >
            <td
              ><input
                type="text"
                aria-label="Home agency, row {i + 1}"
                bind:value={r.home_agency}
                maxlength="120"
                onkeydown={(e) => keydown(e, i)}
              /></td
            >
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
                onclick={() => (rows = rows.filter((_, k) => k !== i))}><Trash2 size={14} /></button
              >
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
    <div class="row wrap">
      <button class="small" onclick={() => add()}><Plus size={14} />Add row</button>
      {#if others.length}
        <span class="spacer"></span>
        <label for="res-copy" class="small">Copy from</label>
        <select id="res-copy" bind:value={copyFrom} style="max-width:220px">
          <option value="">Choose a period…</option>
          {#each others as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
        </select>
        <button class="small" onclick={copy} disabled={!copyFrom}><Copy size={14} />Copy</button>
      {/if}
    </div>
  {:else}<p class="muted">Loading…</p>{/if}
  {#snippet footer()}
    <button onclick={onclose}>Cancel</button>
    <button class="primary" onclick={save} disabled={busy || !loaded}>Save resources</button>
  {/snippet}
</Modal>

<style>
  .res input {
    width: 100%;
  }
  .res td {
    padding: 4px 6px;
  }
</style>
