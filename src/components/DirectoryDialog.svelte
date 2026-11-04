<script lang="ts">
  import { untrack } from "svelte";
  import { CircleAlert } from "@lucide/svelte";
  import Modal from "./Modal.svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import { blankDirectory, KIND_LABEL } from "../lib/directory";
  import type * as T from "../lib/types";

  let {
    item = null,
    initial = null,
    onclose,
    onsaved,
  }: {
    /** Existing item to edit, or null to create. */
    item?: T.DirectoryEntry | null;
    /** Prefill for a new item (Save to Directory from an entry). */
    initial?: Partial<T.DirectoryInput> | null;
    onclose: () => void;
    onsaved?: (e: T.DirectoryEntry) => void;
  } = $props();

  // The dialog edits a copy taken when it opens.
  const start: T.DirectoryInput = untrack(() =>
    item
      ? {
          label: item.label,
          kind: item.kind,
          identifier: item.identifier,
          agency: item.agency,
          channel: item.channel,
          frequency_hz: item.frequency_hz,
          mode: item.mode,
          aliases: [...item.aliases],
          tags: [...item.tags],
          notes: item.notes,
        }
      : { ...blankDirectory(), ...(initial ?? {}) },
  );
  let d = $state<T.DirectoryInput>(start);
  const initialJson = JSON.stringify(start);
  let aliasText = $state(start.aliases.join(", "));
  let tagText = $state(start.tags.join(", "));
  let freqText = $state(start.frequency_hz ? String(start.frequency_hz / 1e6) : "");
  let busy = $state(false);
  let error = $state<T.AppError | null>(null);
  let similar = $state<T.DirectoryEntry[]>([]);
  const err = (f: string) => (error?.field === `directory.${f}` ? error.message : "");
  const split = (s: string) =>
    s
      .split(",")
      .map((x) => x.trim())
      .filter(Boolean);

  async function save() {
    error = null;
    const mhz = freqText.trim() ? Number(freqText.trim()) : null;
    if (mhz !== null && !(mhz > 0)) {
      error = {
        code: "VALIDATION",
        message: "Frequency must be a number of MHz, e.g. 146.52",
        field: "directory.frequency_hz",
      };
      return;
    }
    const input: T.DirectoryInput = {
      ...$state.snapshot(d),
      aliases: split(aliasText),
      tags: split(tagText),
      frequency_hz: mhz === null ? null : Math.round(mhz * 1e6),
    };
    busy = true;
    try {
      const r = await api.saveDirectory(item?.id ?? null, input);
      similar = r.similar;
      app.toast(
        "ok",
        `${r.entry.label} saved to this logbook's directory${r.similar.length ? ` (${r.similar.length} other item(s) use the same identifier)` : ""}.`,
      );
      onsaved?.(r.entry);
      onclose();
    } catch (e) {
      error = toAppError(e);
    } finally {
      busy = false;
    }
  }
</script>

<Modal
  title={item ? "Edit directory item" : "Add to directory"}
  {onclose}
  {busy}
  dirty={() =>
    JSON.stringify({ ...$state.snapshot(d), aliases: split(aliasText), tags: split(tagText) }) !==
    initialJson}
>
  <p class="muted" style="margin:0">
    This logbook's own list of stations you work with. Picking one fills in a new entry; changing it later
    never changes entries already saved. It is not an operator profile and not this logbook's own station.
  </p>
  {#if error && !error.field?.startsWith("directory.")}<div class="banner error" role="alert">
      <CircleAlert size={18} class="icon" />
      <div>{error.message}</div>
    </div>{/if}
  {#if similar.length}<div class="banner warn" role="status">
      Also in the directory with this identifier: {similar.map((s) => s.label).join(", ")}
    </div>{/if}
  <div class="grid-2">
    <div class="field">
      <label for="dir-label">Name shown<span class="req">*</span></label>
      <input
        id="dir-label"
        type="text"
        bind:value={d.label}
        maxlength="120"
        placeholder="e.g. Shelter 2, Net control, Lake County EOC"
        aria-invalid={err("label") ? "true" : undefined}
        data-autofocus
      />
      {#if err("label")}<span class="field-error">{err("label")}</span>{/if}
    </div>
    <div class="field">
      <label for="dir-kind">Type</label>
      <select id="dir-kind" bind:value={d.kind}>
        {#each Object.entries(KIND_LABEL) as [k, l] (k)}<option value={k}>{l}</option>{/each}
      </select>
    </div>
    <div class="field">
      <label for="dir-id">{d.kind === "amateur_callsign" ? "Callsign" : "Identifier"}</label>
      <input
        id="dir-id"
        type="text"
        bind:value={d.identifier}
        maxlength="60"
        class="mono"
        placeholder={d.kind === "amateur_callsign" ? "e.g. W4CLB" : "e.g. SHELTER 2"}
        aria-invalid={err("identifier") ? "true" : undefined}
      />
      {#if err("identifier")}<span class="field-error">{err("identifier")}</span>{/if}
    </div>
    <div class="field">
      <label for="dir-agency">Agency / unit</label>
      <input id="dir-agency" type="text" bind:value={d.agency} maxlength="200" />
    </div>
    <div class="field">
      <label for="dir-chan">Preferred channel</label>
      <input id="dir-chan" type="text" bind:value={d.channel} maxlength="120" placeholder="e.g. TAC 2" />
    </div>
    <div class="grid-2">
      <div class="field">
        <label for="dir-freq">Frequency (MHz)</label>
        <input
          id="dir-freq"
          type="text"
          inputmode="decimal"
          bind:value={freqText}
          class="mono"
          aria-invalid={err("frequency_hz") ? "true" : undefined}
        />
        {#if err("frequency_hz")}<span class="field-error">{err("frequency_hz")}</span>{/if}
      </div>
      <div class="field">
        <label for="dir-mode">Mode</label>
        <input id="dir-mode" type="text" bind:value={d.mode} maxlength="20" class="mono" />
      </div>
    </div>
    <div class="field">
      <label for="dir-alias">Also known as (comma separated)</label>
      <input id="dir-alias" type="text" bind:value={aliasText} placeholder="e.g. Lake HS, Gym" />
    </div>
    <div class="field">
      <label for="dir-tags">Tags (comma separated)</label>
      <input id="dir-tags" type="text" bind:value={tagText} />
    </div>
  </div>
  <div class="field">
    <label for="dir-notes">Notes</label>
    <textarea id="dir-notes" rows="3" bind:value={d.notes} maxlength="2000"></textarea>
  </div>
  {#snippet footer()}
    <button onclick={onclose}>Cancel</button>
    <button class="primary" onclick={save} disabled={busy}>Save</button>
  {/snippet}
</Modal>
