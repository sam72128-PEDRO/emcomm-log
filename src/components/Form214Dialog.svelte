<script lang="ts">
  import { FileDown, LoaderCircle, CircleAlert } from "@lucide/svelte";
  import { untrack } from "svelte";
  import Modal from "./Modal.svelte";
  import FormResultBanner from "./FormResultBanner.svelte";
  import ExportReviewPanel from "./ExportReviewPanel.svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { reviewedRefs } from "../lib/review";
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
  let req = $state<T.Form214Request>({
    op_id:
      untrack(() => opId) ??
      currentPeriod(app.periods)?.id ??
      app.periods.find((x) => x.status === "active")?.id ??
      "",
    name: [p?.name, p?.callsign].filter(Boolean).join(" "),
    ics_position: p?.position ?? "",
    home_agency: p?.organization ?? app.active?.info.incident.agency ?? "",
    kinds: ["activity"],
    include_details: false,
    prepared_by_name: p?.name ?? "",
    prepared_by_position: p?.position ?? "",
    display_zone: app.zone,
    entry_ids: untrack(() => [...entryIds]),
  });
  const KINDS: [T.EntryKind, string][] = [
    ["activity", "Activity"],
    ["message", "Message"],
    ["traffic", "Traffic"],
    ["status", "Status"],
    ["note", "Note"],
  ];
  let preview = $state<T.ExportReview | null>(null);
  const checked = new SvelteSet<string>();
  let previewErr = $state<string | null>(null);
  let busy = $state(false);
  let result = $state<T.FormResult | null>(null);
  let resultErr = $state<string | null>(null);

  $effect(() => {
    const r = $state.snapshot(req) as T.Form214Request;
    preview = null;
    previewErr = null;
    if (!r.op_id) return;
    api
      .formReview("214", r)
      .then((x) => {
        preview = x;
        checked.clear();
        for (const row of x.rows) if (row.eligible) checked.add(row.id);
      })
      .catch((e) => (previewErr = toAppError(e).message));
  });

  function toggleKind(k: T.EntryKind, on: boolean) {
    req.kinds = on ? [...req.kinds, k] : req.kinds.filter((x) => x !== k);
  }

  async function run() {
    const op = app.periods.find((x) => x.id === req.op_id);
    const dest = await api.pickSaveFile(
      "Save ICS 214 Activity Log",
      `ICS 214 - ${app.active!.info.name} - ${op?.name ?? "OP"}.pdf`,
      [{ name: "PDF", extensions: ["pdf"] }],
    );
    if (!dest) return;
    busy = true;
    result = null;
    resultErr = null;
    try {
      result = await api.exportForm214(
        { ...($state.snapshot(req) as T.Form214Request), reviewed: reviewedRefs(preview, checked) },
        dest,
      );
      app.toast("ok", `ICS 214 saved (${result.pages} page${result.pages === 1 ? "" : "s"})`);
    } catch (e) {
      resultErr = toAppError(e).message;
      app.announce(`ICS 214 not saved: ${resultErr}`, true);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title="ICS 214 Activity Log" {onclose} size="lg" {busy}>
  {#if !usable.length}
    <div class="banner warn">
      <CircleAlert size={18} class="icon" />
      <div>An ICS 214 covers one operational period. Add a period on the Dashboard first.</div>
    </div>
  {:else}
    <div class="grid-2">
      <div class="field">
        <label for="f214-op">Operational period<span class="req">*</span></label>
        <select id="f214-op" bind:value={req.op_id}>
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
        <label for="f214-zone">Show times in</label>
        <select id="f214-zone" bind:value={req.display_zone}
          ><option value="local">Local time</option><option value="utc">UTC</option></select
        >
      </div>
    </div>
    <fieldset>
      <legend>Whose log (blocks 3–5)</legend>
      <div class="grid-3">
        <div class="field">
          <label for="f214-name">Name</label><input id="f214-name" type="text" bind:value={req.name} />
        </div>
        <div class="field">
          <label for="f214-pos">ICS position</label><input
            id="f214-pos"
            type="text"
            bind:value={req.ics_position}
          />
        </div>
        <div class="field">
          <label for="f214-ag">Home agency (and unit)</label><input
            id="f214-ag"
            type="text"
            bind:value={req.home_agency}
          />
        </div>
      </div>
    </fieldset>
    <fieldset>
      <legend>Activity log (block 7)</legend>
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
      <span class="hint block"
        >Entries assigned to this period, plus unassigned entries whose time falls inside it. Voided entries
        never appear.</span
      >
    </fieldset>
    <fieldset>
      <legend>Prepared by (block 8 — signature left blank)</legend>
      <div class="grid-2">
        <div class="field">
          <label for="f214-pb">Name</label><input
            id="f214-pb"
            type="text"
            bind:value={req.prepared_by_name}
          />
        </div>
        <div class="field">
          <label for="f214-pp">Position/Title</label><input
            id="f214-pp"
            type="text"
            bind:value={req.prepared_by_position}
          />
        </div>
      </div>
    </fieldset>
    <div class="summary" aria-live="polite">
      {#if previewErr}<span class="field-error"><CircleAlert size={15} /> {previewErr}</span>
      {:else if preview}<ExportReviewPanel review={preview} {checked} />
      {:else}<span class="muted">Counting…</span>{/if}
    </div>
    {#if resultErr}<div class="banner error" role="alert">
        <CircleAlert size={18} class="icon" />
        <div><strong>Not saved.</strong> {resultErr} No partial file was left behind.</div>
      </div>{/if}
    {#if result}<FormResultBanner {result} noun="ICS 214 saved" />{/if}
  {/if}
  {#snippet footer()}
    <button onclick={onclose}>{result ? "Close" : "Cancel"}</button>
    {#if usable.length}
      <button
        class="primary"
        onclick={run}
        disabled={busy ||
          !preview ||
          !!previewErr ||
          !req.kinds.length ||
          (!preview.too_many && checked.size === 0)}
      >
        {#if busy}<LoaderCircle size={16} class="spin" />Saving…{:else}<FileDown size={16} />Save ICS 214…{/if}
      </button>
    {/if}
  {/snippet}
</Modal>

<style>
  .grid-3 {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 10px;
  }
  .summary {
    padding: 8px 0;
  }
  .block {
    display: block;
  }
</style>
