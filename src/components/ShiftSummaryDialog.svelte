<script lang="ts">
  import { Copy, CircleAlert } from "@lucide/svelte";
  import Modal from "./Modal.svelte";
  import HandoverSummaryView from "./HandoverSummaryView.svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import { copyText } from "../lib/clipboard";
  import type * as T from "../lib/types";

  /** 2.3: a past shift's handover, reconstructed from records (never presented as an original). */
  let { sessionId, onclose }: { sessionId: string; onclose: () => void } = $props();
  let summary = $state<T.HandoverSummary | null>(null);
  let error = $state<string | null>(null);

  $effect(() => {
    void sessionId;
    api
      .shiftSummary(sessionId)
      .then((s) => (summary = s))
      .catch((e) => (error = toAppError(e).message));
  });

  async function copy() {
    if (!summary) return;
    if (await copyText(summary.text)) app.toast("ok", "Reconstructed summary copied to the clipboard");
    else app.toast("error", "Could not copy to the clipboard");
  }
  function open(id: string) {
    onclose();
    app.detailEntryId = id;
  }
</script>

<Modal title="Shift summary (reconstructed)" {onclose} size="lg">
  {#if error}<div class="banner error" role="alert">
      <CircleAlert size={18} class="icon" />
      <div>{error}</div>
    </div>{/if}
  {#if summary}<HandoverSummaryView {summary} onopen={open} />{:else if !error}<p class="muted">
      Loading…
    </p>{/if}
  {#snippet footer()}
    <button onclick={copy} disabled={!summary}><Copy size={14} />Copy Summary</button>
    <button class="primary" onclick={onclose}>Close</button>
  {/snippet}
</Modal>
