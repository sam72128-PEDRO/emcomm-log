<script lang="ts">
  // 2.2 Review Export for the generic formats: review first, then choose where to save.
  import { CircleAlert, LoaderCircle } from "@lucide/svelte";
  import { untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import Modal from "./Modal.svelte";
  import ExportReviewPanel from "./ExportReviewPanel.svelte";
  import { api, toAppError } from "../lib/api";
  import type * as T from "../lib/types";

  let {
    request,
    formatLabel,
    onexport,
    onclose,
  }: {
    request: T.ExportRequest;
    formatLabel: string;
    /** Called with the approved entries (null = whole scope, when too many to list). */
    onexport: (reviewed: T.ReviewRef[] | null) => void;
    onclose: () => void;
  } = $props();

  let review = $state<T.ExportReview | null>(null);
  let error = $state<string | null>(null);
  const checked = new SvelteSet<string>();

  // The scope as it was when Review opened; the export is checked against this snapshot.
  api
    .exportReview(untrack(() => $state.snapshot(request)) as T.ExportRequest)
    .then((r) => {
      review = r;
      for (const row of r.rows) if (row.eligible) checked.add(row.id);
    })
    .catch((e) => (error = toAppError(e).message));

  function go() {
    if (!review) return;
    const refs = review.too_many
      ? null
      : review.rows
          .filter((r) => r.eligible && checked.has(r.id))
          .map((r) => ({ id: r.id, revision: r.revision }));
    onexport(refs);
  }
  const n = $derived(review ? (review.too_many ? review.eligible : checked.size) : 0);
</script>

<Modal title="Review export — {formatLabel}" {onclose} size="xl">
  {#if error}<div class="banner error" role="alert"><CircleAlert size={18} class="icon" />{error}</div>
  {:else if !review}<p class="muted"><LoaderCircle size={14} class="spin" /> Listing entries…</p>
  {:else}<ExportReviewPanel {review} {checked} />{/if}
  {#snippet footer()}
    <button onclick={onclose}>Cancel</button>
    <button class="primary" onclick={go} disabled={!review || n === 0}
      >Export {n} entr{n === 1 ? "y" : "ies"}…</button
    >
  {/snippet}
</Modal>
