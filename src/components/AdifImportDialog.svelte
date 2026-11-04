<script lang="ts">
  // 2.2 ADIF import (ADIF-*, DESK-ADIF-*): preview everything first; nothing is written until Import, and
  // then all chosen QSOs are saved together or none are.
  import { CircleAlert, CircleCheck, FileUp, LoaderCircle } from "@lucide/svelte";
  import { SvelteSet } from "svelte/reactivity";
  import Modal from "./Modal.svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import { confirm } from "../lib/confirm.svelte";
  import { formatInstant } from "../lib/time";
  import type * as T from "../lib/types";

  let { onclose }: { onclose: () => void } = $props();

  const PAGE = 100;
  let path = $state<string | null>(null);
  let preview = $state<T.AdifPreview | null>(null);
  let opts = $state<T.AdifImportOptions>({
    default_station_call: app.station?.station_callsign ?? "",
    default_operator_call: "",
  });
  const selected = new SvelteSet<string>();
  let show = $state<"all" | T.AdifRowStatus>("all");
  let page = $state(0);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let result = $state<T.AdifImportResult | null>(null);

  const rows = $derived((preview?.rows ?? []).filter((r) => show === "all" || r.status === show));
  const pageRows = $derived(rows.slice(page * PAGE, page * PAGE + PAGE));
  const pages = $derived(Math.max(1, Math.ceil(rows.length / PAGE)));

  async function choose() {
    error = null;
    const p = await api.pickAdifFile().catch((e) => ((error = toAppError(e).message), null));
    if (!p) return;
    path = p;
    await load();
  }

  async function load() {
    if (!path) return;
    busy = true;
    error = null;
    try {
      preview = await api.adifPreview(path, $state.snapshot(opts));
      // Default: import what is new; potential duplicates are skipped unless chosen.
      selectNew();
      page = 0;
    } catch (e) {
      error = toAppError(e).message;
      preview = null;
    } finally {
      busy = false;
    }
  }

  function toggle(k: string, on: boolean) {
    if (on) selected.add(k);
    else selected.delete(k);
  }
  function selectNew() {
    selected.clear();
    for (const r of preview?.rows ?? []) if (r.status === "ok") selected.add(r.key);
  }

  async function run() {
    if (!preview || !path) return;
    const dups = preview.rows.filter((r) => r.status === "duplicate" && selected.has(r.key)).length;
    const a = await confirm.ask({
      title: "Import QSOs?",
      message: `Add ${selected.size} QSO(s) from ${preview.source_name} to this logbook as new entries?`,
      detail: `${dups ? `${dups} of them look like QSOs already in the log; you chose to import them anyway. ` : ""}All are saved together, or none if anything fails. Existing entries are never changed.`,
      buttons: [
        { id: "cancel", label: "Cancel" },
        { id: "go", label: `Import ${selected.size}`, tone: "primary" },
      ],
      cancelId: "cancel",
    });
    if (a.id !== "go") return;
    busy = true;
    error = null;
    try {
      result = await api.adifImport(path, preview.source_sha256, [...selected], $state.snapshot(opts));
      app.announce(`Imported ${result.imported} QSOs`);
      app.bumpData();
    } catch (e) {
      error = `${toAppError(e).message} Nothing was imported.`;
    } finally {
      busy = false;
    }
  }

  const summaryText = $derived(
    preview && result
      ? [
          `ADIF import — ${preview.source_name}`,
          `File SHA-256: ${preview.source_sha256}`,
          `Program: ${preview.program || "unknown"}; ADIF ${preview.adif_version || "?"}`,
          `QSOs in file: ${preview.records}; new: ${preview.ok}; possible duplicates: ${preview.duplicates}; cannot import: ${preview.invalid}`,
          `Imported: ${result.imported} (entries #${result.first_sequence}–#${result.last_sequence}); not imported: ${result.skipped}`,
          `Batch: ${result.batch_id}`,
          "",
          ...preview.rows
            .filter((r) => !selected.has(r.key) || r.status === "invalid")
            .map((r) => `record ${r.record} ${r.call}: ${r.status}${r.reason ? ` — ${r.reason}` : ""}`),
        ].join("\n")
      : "",
  );
  const fmt = (iso: string | null) => (iso ? formatInstant(iso, "utc", app.clock) : "—");
  const mhz = (hz: number | null) => (hz ? (hz / 1e6).toFixed(4).replace(/0+$/, "").replace(/\.$/, "") : "");
</script>

<Modal title="Import ADIF" {onclose} {busy} size="xl" dirty={() => !!preview && !result}>
  {#if error}<div class="banner error" role="alert"><CircleAlert size={18} class="icon" />{error}</div>{/if}
  {#if result && preview}
    <div class="banner ok" role="status">
      <CircleCheck size={18} class="icon" />
      <div>
        <strong>Imported {result.imported} QSO(s)</strong> as entries #{result.first_sequence}–#{result.last_sequence}.
        {result.skipped} not imported.
      </div>
    </div>
    <label for="adif-sum" class="label">Import summary (copy for your records)</label>
    <textarea id="adif-sum" rows="8" readonly class="mono small">{summaryText}</textarea>
  {:else if !preview}
    <p class="muted" style="margin:0">
      Choose an ADIF file (.adi) exported from QRZ, LoTW, POTA, SOTA or another logging program. It is read on
      this computer only; nothing is uploaded or looked up. ADIF times are UTC.
    </p>
    <div class="row">
      <button class="primary" onclick={choose} disabled={busy}
        >{#if busy}<LoaderCircle size={16} class="spin" />{:else}<FileUp size={16} />{/if}Choose ADIF file…</button
      >
    </div>
  {:else}
    <dl class="kv">
      <dt>File</dt>
      <dd>
        {preview.source_name} · {Math.ceil(preview.size_bytes / 1024)} KB{preview.program
          ? ` · from ${preview.program}`
          : ""}{preview.adif_version ? ` · ADIF ${preview.adif_version}` : ""}
      </dd>
      <dt>QSOs</dt>
      <dd>
        {preview.records} in the file · <strong>{preview.ok} new</strong> · {preview.duplicates} possible duplicate(s)
        ·
        {preview.invalid} cannot be imported
      </dd>
      <dt>Becomes</dt>
      <dd>
        QSO entries with the next numbers; event time = the QSO's UTC start; you are recorded as who imported
        them.
      </dd>
      {#if preview.kept_as_extras.length}<dt>Kept as extras</dt>
        <dd class="small">
          {preview.kept_as_extras.map(([k, n]) => `${k} (${n})`).join(", ")} — stored with each QSO, not shown in
          forms.
        </dd>{/if}
    </dl>
    {#if preview.previous_batches.length}<div class="banner warn" role="status">
        This exact file was imported before ({preview.previous_batches
          .map((b) => `${b.imported} QSOs on ${formatInstant(b.imported_at_utc, app.zone, app.clock)}`)
          .join("; ")}). Its QSOs show as duplicates.
      </div>{/if}
    {#each preview.warnings as w, i (i)}<div class="banner warn">{w}</div>{/each}
    <fieldset>
      <legend>When a QSO has no station or operator callsign</legend>
      <div class="grid-2">
        <div class="field">
          <label for="adif-st">Station callsign to use</label>
          <input
            id="adif-st"
            type="text"
            class="mono"
            bind:value={opts.default_station_call}
            maxlength="20"
          />
        </div>
        <div class="field">
          <label for="adif-op">Operator callsign to use</label>
          <input
            id="adif-op"
            type="text"
            class="mono"
            bind:value={opts.default_operator_call}
            maxlength="20"
          />
        </div>
      </div>
      <div class="row">
        <span class="hint"
          >Values in the file always win. Your profile and this logbook's station are not changed.</span
        >
        <span class="spacer"></span>
        <button class="small" onclick={load} disabled={busy}>Apply</button>
      </div>
    </fieldset>
    <div class="row wrap">
      <select bind:value={show} onchange={() => (page = 0)} aria-label="Show">
        <option value="all">All ({preview.rows.length})</option>
        <option value="ok">New ({preview.ok})</option>
        <option value="duplicate">Possible duplicates ({preview.duplicates})</option>
        <option value="invalid">Cannot import ({preview.invalid})</option>
      </select>
      <button class="small" onclick={selectNew}>Select new only</button>
      <button class="small" onclick={() => selected.clear()}>Deselect all</button>
      <span class="spacer"></span>
      <strong>{selected.size} selected</strong>
    </div>
    <div class="table-wrap">
      <table class="table small">
        <thead
          ><tr
            ><th></th><th>#</th><th>UTC start</th><th>Call</th><th>Band / MHz</th><th>Mode</th><th
              >Station / Op</th
            ><th>Park / Summit</th><th>Status</th></tr
          ></thead
        >
        <tbody>
          {#each pageRows as r (r.key)}
            <tr class={r.status}>
              <td
                ><input
                  type="checkbox"
                  aria-label="Import record {r.record} {r.call}"
                  disabled={r.status === "invalid"}
                  checked={selected.has(r.key)}
                  onchange={(e) => toggle(r.key, e.currentTarget.checked)}
                /></td
              >
              <td>{r.record}</td>
              <td class="mono">{fmt(r.start_utc)}</td>
              <td class="mono">{r.call}</td>
              <td>{r.band}{r.frequency_hz ? ` · ${mhz(r.frequency_hz)}` : ""}</td>
              <td>{r.mode}</td>
              <td class="mono">{r.station_call}{r.operator_call ? ` / ${r.operator_call}` : ""}</td>
              <td class="mono"
                >{[
                  r.my_pota_ref && `my ${r.my_pota_ref}`,
                  r.pota_ref,
                  r.my_sota_ref && `my ${r.my_sota_ref}`,
                  r.sota_ref,
                ]
                  .filter(Boolean)
                  .join(" · ")}</td
              >
              <td
                >{#if r.status === "ok"}<span class="badge ok">New</span
                  >{:else if r.status === "duplicate"}<span class="badge warn">Duplicate?</span>{:else}<span
                    class="badge danger">Cannot import</span
                  >{/if}
                {#if r.reason}<div class="muted">{r.reason}</div>{/if}
                {#each r.warnings as w, i (i)}<div class="muted">{w}</div>{/each}</td
              >
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    {#if pages > 1}<div class="row">
        <button class="small" disabled={page === 0} onclick={() => page--}>Previous</button>
        <span class="muted">Page {page + 1} of {pages}</span>
        <button class="small" disabled={page >= pages - 1} onclick={() => page++}>Next</button>
      </div>{/if}
  {/if}
  {#snippet footer()}
    {#if result}<button class="primary" onclick={onclose}>Done</button>
    {:else}
      <button onclick={onclose}>Cancel</button>
      {#if preview}<button class="primary" onclick={run} disabled={busy || selected.size === 0}
          >{#if busy}<LoaderCircle size={16} class="spin" />{/if}Import {selected.size} QSO(s)…</button
        >{/if}
    {/if}
  {/snippet}
</Modal>

<style>
  .table-wrap {
    max-height: 46vh;
    overflow: auto;
  }
  tr.invalid {
    opacity: 0.7;
  }
  .small {
    font-size: 12.5px;
  }
  textarea {
    width: 100%;
  }
</style>
