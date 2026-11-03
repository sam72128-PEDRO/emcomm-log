<script lang="ts">
  import { FileDown, LoaderCircle, CircleAlert } from "@lucide/svelte";
  import { untrack } from "svelte";
  import Modal from "./Modal.svelte";
  import FormResultBanner from "./FormResultBanner.svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import { currentPeriod } from "../lib/periods";
  import { formatInstant } from "../lib/time";
  import type * as T from "../lib/types";

  let {
    opId = null,
    entryIds = [],
    onclose,
  }: { opId?: string | null; entryIds?: string[]; onclose: () => void } = $props();

  const usable = $derived(app.periods.filter((p) => p.status === "active"));
  const p = app.profile;
  let req = $state<T.Form309Request>({
    op_id:
      untrack(() => opId) ??
      currentPeriod(app.periods)?.id ??
      app.periods.find((x) => x.status === "active")?.id ??
      "",
    entry_ids: untrack(() => [...entryIds]),
    kinds: ["traffic", "message"],
    include_details: false,
    net_name: "",
    operator_name: p?.name ?? "",
    operator_call: p?.callsign ?? "",
    prepared_by_name: p?.name ?? "",
    prepared_by_call: p?.callsign ?? "",
    display_zone: app.zone,
  });
  const KINDS: [T.EntryKind, string][] = [
    ["traffic", "Traffic"],
    ["message", "Message"],
    ["status", "Status"],
    ["note", "Note"],
    ["activity", "Activity"],
  ];
  let preview = $state<T.FormPreview | null>(null);
  let previewErr = $state<string | null>(null);
  let busy = $state(false);
  let result = $state<T.FormResult | null>(null);
  let resultErr = $state<string | null>(null);

  $effect(() => {
    const r = $state.snapshot(req) as T.Form309Request;
    preview = null;
    previewErr = null;
    if (!r.op_id) return;
    api
      .form309Preview(r)
      .then((x) => (preview = x))
      .catch((e) => (previewErr = toAppError(e).message));
  });

  function toggleKind(k: T.EntryKind, on: boolean) {
    req.kinds = on ? [...req.kinds, k] : req.kinds.filter((x) => x !== k);
  }

  async function run() {
    const op = app.periods.find((x) => x.id === req.op_id);
    const dest = await api.pickSaveFile(
      "Save ICS 309 Communications Log",
      `ICS 309 - ${app.active!.info.name} - ${op?.name ?? "OP"}.pdf`,
      [{ name: "PDF", extensions: ["pdf"] }],
    );
    if (!dest) return;
    busy = true;
    result = null;
    resultErr = null;
    try {
      result = await api.exportForm309($state.snapshot(req) as T.Form309Request, dest);
      app.toast("ok", `ICS 309 saved (${result.pages} page${result.pages === 1 ? "" : "s"})`);
    } catch (e) {
      resultErr = toAppError(e).message;
      app.announce(`ICS 309 not saved: ${resultErr}`, true);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title="ICS 309 Communications Log" {onclose} size="lg" {busy}>
  {#if !usable.length}
    <div class="banner warn">
      <CircleAlert size={18} class="icon" />
      <div>An ICS 309 covers one operational period. Add a period on the Dashboard first.</div>
    </div>
  {:else}
    <div class="grid-2">
      <div class="field">
        <label for="f309-op">Operational period<span class="req">*</span></label>
        <select id="f309-op" bind:value={req.op_id}>
          {#each usable as op (op.id)}
            <option value={op.id}
              >{op.name} — {formatInstant(op.start_utc, app.zone, app.clock)} to {formatInstant(
                op.end_utc,
                app.zone,
                app.clock,
              )}</option
            >
          {/each}
        </select>
      </div>
      <div class="field">
        <label for="f309-zone">Show times in</label>
        <select id="f309-zone" bind:value={req.display_zone}
          ><option value="local">Local time</option><option value="utc">UTC</option></select
        >
      </div>
    </div>
    <fieldset>
      <legend>Net and operator (blocks 3–4)</legend>
      <div class="grid-3">
        <div class="field">
          <label for="f309-net">Radio net name or position/tactical call</label><input
            id="f309-net"
            type="text"
            bind:value={req.net_name}
            placeholder="e.g. Lake County ARES Net"
          />
        </div>
        <div class="field">
          <label for="f309-on">Radio operator — name</label><input
            id="f309-on"
            type="text"
            bind:value={req.operator_name}
          />
        </div>
        <div class="field">
          <label for="f309-oc">Radio operator — call sign</label><input
            id="f309-oc"
            type="text"
            bind:value={req.operator_call}
          />
        </div>
      </div>
    </fieldset>
    <fieldset>
      <legend>Communications log (block 5)</legend>
      <div class="row wrap">
        {#each KINDS as [k, label] (k)}
          <label class="check"
            ><input
              type="checkbox"
              checked={req.kinds.includes(k)}
              onchange={(e) => toggleKind(k, e.currentTarget.checked)}
            />
            {label}</label
          >
        {/each}
      </div>
      <label class="check"
        ><input type="checkbox" bind:checked={req.include_details} /> Include each entry's details after its summary</label
      >
      <span class="hint block">
        {#if req.entry_ids.length}Only the {req.entry_ids.length} entries selected in the Traffic Log.{:else}Every
          entry assigned to this period, plus unassigned entries whose time falls inside it.{/if}
        Received message numbers go under FROM, sent ones under TO. Voided entries never appear.
      </span>
      {#if req.entry_ids.length}<button class="small link" onclick={() => (req.entry_ids = [])}
          >Use the whole period instead</button
        >{/if}
    </fieldset>
    <fieldset>
      <legend>Prepared by (block 6)</legend>
      <div class="grid-2">
        <div class="field">
          <label for="f309-pb">Name</label><input
            id="f309-pb"
            type="text"
            bind:value={req.prepared_by_name}
          />
        </div>
        <div class="field">
          <label for="f309-pc">Call sign</label><input
            id="f309-pc"
            type="text"
            bind:value={req.prepared_by_call}
          />
        </div>
      </div>
    </fieldset>
    <div class="summary" aria-live="polite">
      {#if previewErr}<span class="field-error"><CircleAlert size={15} /> {previewErr}</span>
      {:else if preview}<strong>{preview.count} entr{preview.count === 1 ? "y" : "ies"}</strong> will be
        logged.
        {#if preview.skipped.length}
          <details>
            <summary
              >{preview.skipped.length} selected entr{preview.skipped.length === 1 ? "y" : "ies"} left out</summary
            >
            <ul>
              {#each preview.skipped as s (s.sequence_no)}<li>#{s.sequence_no}: {s.reason}</li>{/each}
            </ul>
          </details>
        {/if}
      {:else}<span class="muted">Counting…</span>{/if}
    </div>
    {#if resultErr}<div class="banner error" role="alert">
        <CircleAlert size={18} class="icon" />
        <div><strong>Not saved.</strong> {resultErr} No partial file was left behind.</div>
      </div>{/if}
    {#if result}<FormResultBanner {result} noun="ICS 309 saved" />{/if}
  {/if}
  {#snippet footer()}
    <button onclick={onclose}>{result ? "Close" : "Cancel"}</button>
    {#if usable.length}
      <button class="primary" onclick={run} disabled={busy || !preview || !!previewErr || !req.kinds.length}>
        {#if busy}<LoaderCircle size={16} class="spin" />Saving…{:else}<FileDown size={16} />Save ICS 309…{/if}
      </button>
    {/if}
  {/snippet}
</Modal>

<style>
  .grid-3 {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 10px;
  }
  .summary {
    padding: 8px 0;
  }
  .block {
    display: block;
  }
</style>
