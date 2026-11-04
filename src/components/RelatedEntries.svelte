<script lang="ts">
  import { Link2, Plus, X } from "@lucide/svelte";
  import LinkEntryDialog from "./LinkEntryDialog.svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import { confirm } from "../lib/confirm.svelte";
  import { linkLabel } from "../lib/links";
  import { formatInstant } from "../lib/time";
  import type * as T from "../lib/types";

  let { entry }: { entry: T.Entry } = $props();
  let links = $state<T.LinkView[]>([]);
  let showRemoved = $state(false);
  let linking = $state(false);

  async function load() {
    try {
      links = await api.entryLinks(entry.id, showRemoved);
    } catch (e) {
      app.toast("error", toAppError(e).message);
    }
  }
  $effect(() => {
    void entry.id;
    void showRemoved;
    void app.dataVersion;
    void load();
  });

  async function remove(v: T.LinkView) {
    const a = await confirm.ask({
      title: "Remove link?",
      message: `Remove "${linkLabel(v)} #${v.counterpart.sequence_no}"?`,
      detail: "Both entries stay exactly as they are. The removal is recorded with your name and the time.",
      buttons: [
        { id: "cancel", label: "Cancel" },
        { id: "remove", label: "Remove link", tone: "danger" },
      ],
      cancelId: "cancel",
    });
    if (a.id !== "remove") return;
    try {
      await api.removeLink(v.link.id);
      app.announce("Link removed");
      app.bumpData();
    } catch (e) {
      app.toast("error", toAppError(e).message);
    }
  }
</script>

<fieldset>
  <legend><Link2 size={14} /> Related entries</legend>
  {#if links.length}
    <ul class="links">
      {#each links as v (v.link.id)}
        <li class:removed={!!v.link.removed_at_utc}>
          <span class="rel">{linkLabel(v)}</span>
          <button class="link" onclick={() => (app.detailEntryId = v.counterpart.id)}
            >#{v.counterpart.sequence_no}</button
          >
          <span class="muted small">{formatInstant(v.counterpart.event_at_utc, app.zone, app.clock)}</span>
          <span class="subj">{v.counterpart.subject}</span>
          {#if v.counterpart.status === "void"}<span class="badge danger">VOIDED</span>{/if}
          {#if v.link.note}<span class="muted small">— {v.link.note}</span>{/if}
          {#if v.link.removed_at_utc}<span class="muted small"
              >removed {formatInstant(v.link.removed_at_utc, app.zone, app.clock)} by {v.link.removed_by
                ?.name ?? "unknown"}</span
            >{:else if !app.readOnly}<button
              class="ghost icon-only small"
              aria-label="Remove link to #{v.counterpart.sequence_no}"
              onclick={() => remove(v)}><X size={14} /></button
            >{/if}
        </li>
      {/each}
    </ul>
  {:else}<p class="muted small" style="margin:0">No related entries.</p>{/if}
  <div class="row wrap">
    {#if !app.readOnly}<button class="small" onclick={() => (linking = true)}
        ><Plus size={14} />Link existing entry…</button
      >{/if}
    <label class="check small"><input type="checkbox" bind:checked={showRemoved} /> Show removed links</label>
  </div>
</fieldset>

{#if linking}<LinkEntryDialog {entry} onclose={() => (linking = false)} />{/if}

<style>
  .links {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .links li {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .links li.removed {
    opacity: 0.6;
  }
  .rel {
    font-weight: 600;
    min-width: 110px;
  }
  .subj {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 320px;
  }
  .small {
    font-size: 12.5px;
  }
</style>
