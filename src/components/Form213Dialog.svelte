<script lang="ts">
  import { FileDown, LoaderCircle, CircleAlert } from "@lucide/svelte";
  import { untrack } from "svelte";
  import Modal from "./Modal.svelte";
  import FormResultBanner from "./FormResultBanner.svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import type * as T from "../lib/types";

  let { entryIds, onclose }: { entryIds: string[]; onclose: () => void } = $props();

  let zone = $state<"local" | "utc">(app.zone);
  let preview = $state<T.FormPreview | null>(null);
  let previewErr = $state<string | null>(null);
  let busy = $state(false);
  let result = $state<T.FormResult | null>(null);
  let resultErr = $state<string | null>(null);

  api
    .form213Preview(untrack(() => ({ entry_ids: entryIds, display_zone: zone })))
    .then((p) => (preview = p))
    .catch((e) => (previewErr = toAppError(e).message));

  async function run() {
    const stamp = new Date().toISOString().slice(0, 10);
    const dest = await api.pickSaveFile(
      "Save ICS 213 General Message",
      `ICS 213 - ${app.active!.info.name} ${stamp}.pdf`,
      [{ name: "PDF", extensions: ["pdf"] }],
    );
    if (!dest) return;
    busy = true;
    result = null;
    resultErr = null;
    try {
      result = await api.exportForm213({ entry_ids: entryIds, display_zone: zone }, dest);
      app.toast("ok", `ICS 213 saved (${result.exported} message${result.exported === 1 ? "" : "s"})`);
    } catch (e) {
      resultErr = toAppError(e).message;
      app.announce(`ICS 213 not saved: ${resultErr}`, true);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title="ICS 213 General Message" {onclose} {busy}>
  <p class="muted" style="margin:0">
    One filled ICS 213 per message, in entry order, on FEMA's official form. Signature lines are left blank
    for signing after printing. The fields stay editable in any PDF reader.
  </p>
  <div class="field">
    <label for="f213-zone">Show times in</label>
    <select id="f213-zone" bind:value={zone} style="max-width:200px"
      ><option value="local">Local time</option><option value="utc">UTC</option></select
    >
  </div>
  <div aria-live="polite">
    {#if previewErr}<span class="field-error"><CircleAlert size={15} /> {previewErr}</span>
    {:else if preview}
      <strong>{preview.count} message{preview.count === 1 ? "" : "s"}</strong> will be filled in.
      {#if preview.skipped.length}
        <details open={preview.count === 0}>
          <summary
            >{preview.skipped.length} selected entr{preview.skipped.length === 1 ? "y" : "ies"} can't go on an ICS
            213</summary
          >
          <ul>
            {#each preview.skipped as s (s.sequence_no)}<li>#{s.sequence_no}: {s.reason}</li>{/each}
          </ul>
        </details>
      {/if}
    {:else}<span class="muted">Checking the selection…</span>{/if}
  </div>
  {#if resultErr}<div class="banner error" role="alert">
      <CircleAlert size={18} class="icon" />
      <div><strong>Not saved.</strong> {resultErr} No partial file was left behind.</div>
    </div>{/if}
  {#if result}<FormResultBanner {result} noun="ICS 213 saved" />{/if}
  {#snippet footer()}
    <button onclick={onclose}>{result ? "Close" : "Cancel"}</button>
    <button class="primary" onclick={run} disabled={busy || !preview || preview.count === 0}>
      {#if busy}<LoaderCircle size={16} class="spin" />Saving…{:else}<FileDown size={16} />Save ICS 213…{/if}
    </button>
  {/snippet}
</Modal>
