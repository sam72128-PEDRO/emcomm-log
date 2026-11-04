<script lang="ts">
  // 2.2 Review Export (EXP-01..03): exactly which entries the file or form will contain. Selection is by
  // entry id; reviewing never changes an entry.
  import { SvelteSet } from "svelte/reactivity";
  import { app } from "../lib/state.svelte";
  import { formatInstant } from "../lib/time";
  import type * as T from "../lib/types";

  let {
    review,
    checked,
    label = "entries",
  }: {
    review: T.ExportReview;
    /** Ids chosen for export (owned by the parent). */
    checked: SvelteSet<string>;
    label?: string;
  } = $props();

  let show = $state<"all" | "in" | "out">("all");
  const rows = $derived(
    review.rows.filter((r) =>
      show === "all"
        ? true
        : show === "in"
          ? r.eligible && checked.has(r.id)
          : !r.eligible || !checked.has(r.id),
    ),
  );
  const LIMIT = 500;
  let limit = $state(LIMIT);

  function selectEligible() {
    checked.clear();
    for (const r of review.rows) if (r.eligible) checked.add(r.id);
  }
  function open(r: T.ReviewRow) {
    app.detailEntryId = r.id;
  }
</script>

<div class="review col">
  {#each review.notes as n, i (i)}<p class="hint" style="margin:0">{n}</p>{/each}
  {#if review.too_many}
    <div class="banner info" role="status">
      {review.total.toLocaleString()} entries ({review.eligible.toLocaleString()} can be included): too many to
      list one by one. The whole scope is exported; narrow the date range or filter to choose individual entries.
    </div>
  {:else}
    <div class="row wrap" aria-live="polite">
      <strong>{checked.size} of {review.eligible} {label} will be included</strong>
      {#if review.excluded}<span class="muted">· {review.excluded} cannot be included (reasons below)</span
        >{/if}
      <span class="spacer"></span>
      <select bind:value={show} aria-label="Show">
        <option value="all">All ({review.rows.length})</option>
        <option value="in">Included ({checked.size})</option>
        <option value="out">Left out ({review.rows.length - checked.size})</option>
      </select>
      <button class="small" onclick={selectEligible}>Select all eligible</button>
      <button class="small" onclick={() => checked.clear()}>Deselect all</button>
    </div>
    <div class="table-wrap">
      <table class="table small">
        <thead
          ><tr
            ><th></th><th>#</th><th>Time</th><th>Type</th><th>Station</th><th>Summary</th><th>Operator</th><th
              >Note</th
            ></tr
          ></thead
        >
        <tbody>
          {#each rows.slice(0, limit) as r (r.id)}
            <tr
              class:out={!r.eligible}
              tabindex="0"
              onkeydown={(e) => {
                if (e.key === "Enter" && e.target === e.currentTarget) open(r);
              }}
            >
              <td
                ><input
                  type="checkbox"
                  aria-label="Include entry {r.sequence_no}"
                  disabled={!r.eligible}
                  checked={r.eligible && checked.has(r.id)}
                  onchange={(e) => (e.currentTarget.checked ? checked.add(r.id) : checked.delete(r.id))}
                /></td
              >
              <td><button class="link" onclick={() => open(r)}>#{r.sequence_no}</button></td>
              <td class="mono">{formatInstant(r.event_at_utc, app.zone, app.clock)}</td>
              <td>{r.kind}</td>
              <td>{r.station}</td>
              <td class="subj">{r.subject}{r.attachments ? ` · ${r.attachments} attachment(s)` : ""}</td>
              <td>{r.operator}</td>
              <td
                >{#if !r.eligible}<span class="badge danger">Not included</span>
                  <span class="muted">{r.reason}</span>{:else if !checked.has(r.id)}<span class="muted"
                    >Left out</span
                  >{/if}</td
              >
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    {#if rows.length > limit}<button class="small" onclick={() => (limit += LIMIT)}
        >Show {Math.min(LIMIT, rows.length - limit)} more</button
      >{/if}
    <span class="hint"
      >Enter on a row (or its number) opens the entry. Choosing entries here never changes them.</span
    >
  {/if}
</div>

<style>
  .table-wrap {
    max-height: 42vh;
    overflow: auto;
  }
  tr.out {
    opacity: 0.65;
  }
  .subj {
    max-width: 320px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .small {
    font-size: 12.5px;
  }
</style>
