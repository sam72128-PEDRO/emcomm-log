<script lang="ts">
  import { CircleAlert, Search } from "@lucide/svelte";
  import Modal from "./Modal.svelte";
  import { app, defaultQuery } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import { RELATION_CHOICES } from "../lib/links";
  import { formatInstant } from "../lib/time";
  import type * as T from "../lib/types";

  let { entry, onclose }: { entry: T.Entry; onclose: () => void } = $props();

  let text = $state("");
  let results = $state<T.Entry[]>([]);
  let target = $state<T.Entry | null>(null);
  let relation = $state<T.Relation>("related_to");
  let note = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);
  let active = $state(0);

  // Search this logbook only: "#12" or "12" finds entry 12; anything else searches text, station, callsign,
  // message number. Most recent first.
  let seq = 0;
  $effect(() => {
    const t = text.trim();
    const my = ++seq;
    const num = /^#?(\d+)$/.exec(t);
    const q: T.EntryQuery = {
      ...defaultQuery(),
      status: "all",
      sort: "recorded",
      descending: true,
      limit: 30,
    };
    const run = num ? api.listEntries({ ...q, limit: 500 }) : api.listEntries({ ...q, text: t });
    run
      .then((p) => {
        if (my !== seq) return;
        const items = p.items.filter((e) => e.id !== entry.id);
        results = num ? items.filter((e) => String(e.sequence_no).startsWith(num[1])).slice(0, 30) : items;
        active = 0;
      })
      .catch((e) => (error = toAppError(e).message));
  });

  async function link() {
    if (!target) return;
    busy = true;
    error = null;
    try {
      await api.addLink(entry.id, target.id, relation, note);
      app.announce(`Linked to entry ${target.sequence_no}`);
      app.bumpData();
      onclose();
    } catch (e) {
      error = toAppError(e).message;
    } finally {
      busy = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (target || !results.length) return;
    if (e.key === "ArrowDown") {
      active = Math.min(active + 1, results.length - 1);
      e.preventDefault();
    } else if (e.key === "ArrowUp") {
      active = Math.max(active - 1, 0);
      e.preventDefault();
    } else if (e.key === "Enter") {
      target = results[active];
      e.preventDefault();
    }
  }
</script>

<Modal title="Link entry #{entry.sequence_no} to another entry" {onclose} {busy} dirty={() => !!target}>
  {#if error}<div class="banner error" role="alert"><CircleAlert size={18} class="icon" />{error}</div>{/if}
  {#if !target}
    <div class="field">
      <label for="lk-search">Find the other entry in this logbook</label>
      <div class="row">
        <Search size={16} aria-hidden="true" />
        <input
          id="lk-search"
          type="search"
          bind:value={text}
          onkeydown={onKey}
          placeholder="Entry number, station, callsign, words, message number"
          data-autofocus
        />
      </div>
      <span class="hint">↑ ↓ to move, Enter to choose. Most recent first.</span>
    </div>
    <ul class="results" role="listbox" aria-label="Entries">
      {#each results as e, i (e.id)}
        <li role="option" aria-selected={i === active}>
          <button class="ghost" class:active={i === active} onclick={() => (target = e)}>
            <strong>#{e.sequence_no}</strong>
            <span class="muted">{formatInstant(e.event_at_utc, app.zone, app.clock)}</span>
            <span>{e.from_text || e.ham?.their_call || ""}{e.to_text ? ` → ${e.to_text}` : ""}</span>
            <span class="subj">{e.subject}</span>
            {#if e.status === "void"}<span class="badge danger">VOIDED</span>{/if}
          </button>
        </li>
      {:else}<li class="muted">No matching entries.</li>{/each}
    </ul>
  {:else}
    <p style="margin:0">
      Entry <strong>#{entry.sequence_no}</strong> ({entry.subject}) is a…
    </p>
    <div class="grid-2">
      <div class="field">
        <label for="lk-rel">Relationship</label>
        <select id="lk-rel" bind:value={relation}>
          {#each RELATION_CHOICES as c (c.value)}<option value={c.value}>{c.label}</option>{/each}
        </select>
        <span class="hint">{RELATION_CHOICES.find((c) => c.value === relation)?.hint}</span>
      </div>
      <div class="field">
        <span class="label">…of entry</span>
        <strong>#{target.sequence_no} · {target.subject}</strong>
        <button class="small link" onclick={() => (target = null)}>Choose a different entry</button>
      </div>
    </div>
    <div class="field">
      <label for="lk-note">Note (optional)</label>
      <input
        id="lk-note"
        type="text"
        bind:value={note}
        maxlength="200"
        placeholder="e.g. relayed via MARC-5"
      />
    </div>
    <p class="hint" style="margin:0">
      Linking changes neither entry and adds no entry to the log. It can be removed later; the removal is
      recorded.
    </p>
  {/if}
  {#snippet footer()}
    <button onclick={onclose}>Cancel</button>
    <button class="primary" onclick={link} disabled={!target || busy}>Link</button>
  {/snippet}
</Modal>

<style>
  .results {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 320px;
    overflow: auto;
  }
  .results button {
    display: flex;
    gap: 10px;
    width: 100%;
    text-align: left;
    align-items: center;
  }
  .results button.active {
    background: color-mix(in srgb, var(--accent) 18%, transparent);
  }
  .subj {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
