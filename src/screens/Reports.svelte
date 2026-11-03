<script lang="ts">
  import {
    ChartColumn,
    FileDown,
    PackagePlus,
    FolderSearch,
    CircleAlert,
    CircleCheck,
    LoaderCircle,
    ExternalLink,
  } from "@lucide/svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import type * as T from "../lib/types";
  import { todayDateString, zoneLabel } from "../lib/time";

  const info = app.active!.info;
  const formats: { id: T.ExportFormat; label: string; desc: string; ext: string; ham?: boolean }[] = [
    { id: "pdf", label: "PDF", desc: "Print-ready report with page numbers", ext: "pdf" },
    { id: "docx", label: "Word (DOCX)", desc: "Editable report document", ext: "docx" },
    { id: "xlsx", label: "Excel (XLSX)", desc: "Spreadsheet with filters and typed UTC times", ext: "xlsx" },
    { id: "csv", label: "CSV", desc: "UTF-8 comma-separated values", ext: "csv" },
    { id: "txt", label: "Text (TXT)", desc: "Readable chronological log", ext: "txt" },
    { id: "adif", label: "ADIF (.adi)", desc: "Qualifying QSOs only, ADIF 3.1.4", ext: "adi", ham: true },
  ];
  const available = formats.filter((f) => !f.ham || info.logbook_type === "ham");

  let format = $state<T.ExportFormat>("pdf");
  let scope = $state<"all" | "range" | "filter">(app.reportScope);
  let from = $state(todayDateString(app.zone));
  let to = $state(todayDateString(app.zone));
  let includeVoided = $state(app.reportScope === "filter" ? app.logQuery.status !== "active" : false);
  let zone = $state<"local" | "utc">(app.zone);
  let preview = $state<T.ExportPreview | null>(null);
  let previewErr = $state<string | null>(null);
  let running = $state(false);
  let result = $state<T.ExportResult | null>(null);
  let resultErr = $state<string | null>(null);
  let pkgBusy = $state(false);
  let pkgResult = $state<T.ExportPackageResult | null>(null);

  const request = $derived<T.ExportRequest>({
    format,
    scope,
    range_from: scope === "range" ? from : null,
    range_to: scope === "range" ? to : null,
    filter: scope === "filter" ? { ...app.logQuery, limit: 0, offset: 0 } : null,
    include_voided: includeVoided,
    display_zone: zone,
  });

  $effect(() => {
    const r = request;
    void app.dataVersion;
    preview = null;
    previewErr = null;
    api
      .exportPreview(r)
      .then((p) => (preview = p))
      .catch((e) => (previewErr = toAppError(e).message));
  });

  async function run() {
    const f = formats.find((x) => x.id === format)!;
    const stamp = new Date().toISOString().slice(0, 10);
    const dest = await api.pickSaveFile(`Export ${f.label}`, `${info.name} ${stamp}.${f.ext}`, [
      { name: f.label, extensions: [f.ext] },
    ]);
    if (!dest) return;
    running = true;
    result = null;
    resultErr = null;
    try {
      result = await api.runExport(request, dest);
      app.toast("ok", `Export complete: ${result.exported} record(s) written.`);
    } catch (e) {
      resultErr = toAppError(e).message;
      app.announce(`Export failed: ${resultErr}`, true);
    } finally {
      running = false;
    }
  }

  async function exportPackage() {
    const dest = await api.pickSaveFile("Export portable logbook package", `${info.name}.emlog`, [
      { name: "EMCOMM LOG package", extensions: ["emlog"] },
    ]);
    if (!dest) return;
    pkgBusy = true;
    pkgResult = null;
    try {
      pkgResult = await api.exportPackage(null, dest);
      app.toast("ok", "Package exported and verified.");
    } catch (e) {
      app.toast("error", `Package export failed: ${toAppError(e).message}`);
    } finally {
      pkgBusy = false;
    }
  }

  const filterDescription = $derived(scope === "filter" && preview ? preview.description : "");
</script>

<div class="page">
  <section class="panel">
    <div class="panel-head">
      <h1 class="title"><ChartColumn size={22} class="icon" />Reports &amp; Export</h1>
    </div>
    <div class="panel-body col">
      <fieldset>
        <legend>Format</legend>
        <div class="formats" role="radiogroup" aria-label="Export format">
          {#each available as f (f.id)}
            <label class="fmt" class:sel={format === f.id}>
              <input type="radio" name="fmt" value={f.id} bind:group={format} class="sr-only" />
              <strong>{f.label}</strong><span class="muted">{f.desc}</span>
            </label>
          {/each}
        </div>
      </fieldset>
      <div class="grid-2">
        <fieldset>
          <legend>Which entries</legend>
          <label class="check"
            ><input type="radio" name="scope" value="all" bind:group={scope} /> Entire logbook</label
          >
          <label class="check"
            ><input type="radio" name="scope" value="range" bind:group={scope} /> Date range</label
          >
          {#if scope === "range"}
            <div class="row" style="padding-left:24px">
              <label for="rp-from">From</label><input
                id="rp-from"
                type="date"
                bind:value={from}
                style="max-width:170px"
              />
              <label for="rp-to">To</label><input
                id="rp-to"
                type="date"
                bind:value={to}
                style="max-width:170px"
              />
            </div>
            <span class="hint" style="padding-left:24px"
              >Inclusive dates in {zoneLabel(zone)}; converted to exact UTC boundaries.</span
            >
          {/if}
          <label class="check"
            ><input type="radio" name="scope" value="filter" bind:group={scope} /> Current Traffic Log filter{#if app.logQuery.ids.length}
              ({app.logQuery.ids.length} selected entries){/if}</label
          >
          {#if scope === "filter"}<span class="hint" style="padding-left:24px">{filterDescription}</span>{/if}
        </fieldset>
        <fieldset>
          <legend>Options</legend>
          <label class="check"
            ><input type="checkbox" bind:checked={includeVoided} /> Include voided entries</label
          >
          <div class="field">
            <label for="rp-zone">Show times in</label>
            <select id="rp-zone" bind:value={zone}
              ><option value="local">Local time</option><option value="utc">UTC</option></select
            >
          </div>
          <span class="hint"
            >Every report includes the logbook name and type, generated time and zone, the scope description,
            entry numbers, event and recorded times, and status.</span
          >
        </fieldset>
      </div>
      <div class="summary" aria-live="polite">
        {#if previewErr}<span class="field-error"><CircleAlert size={15} /> {previewErr}</span>
        {:else if preview}
          <strong>{preview.count} record(s)</strong> · {formats.find((f) => f.id === format)?.label}
          {#if preview.adif_eligible !== null}
            · <strong>{preview.adif_eligible}</strong> eligible QSO(s), {preview.adif_skipped} will be skipped with
            reasons{/if}
          <div class="hint">{preview.description}</div>
        {:else}<span class="muted">Counting…</span>{/if}
      </div>
      <div class="row">
        <button
          class="primary"
          onclick={run}
          disabled={running ||
            !!previewErr ||
            !preview ||
            (format === "adif" ? preview.adif_eligible === 0 : preview.count === 0)}
        >
          {#if running}<LoaderCircle size={16} class="spin" />Exporting…{:else}<FileDown size={16} />Export {formats.find(
              (f) => f.id === format,
            )?.label}…{/if}
        </button>
        <span class="hint"
          >No Office installation needed. Files are written to a temporary name and only renamed into place
          when complete.</span
        >
      </div>
      {#if resultErr}<div class="banner error" role="alert">
          <CircleAlert size={18} class="icon" />
          <div><strong>Export failed.</strong> {resultErr} No partial file was left behind.</div>
        </div>{/if}
      {#if result}
        <div class="banner ok">
          <CircleCheck size={18} class="icon" />
          <div class="grow">
            <strong>Exported {result.exported} record(s)</strong> to <span class="mono">{result.path}</span>
            ({Math.ceil(result.bytes / 1024)} KB).
            {#if result.skipped.length}
              <details>
                <summary
                  >{result.skipped.length} entr{result.skipped.length === 1 ? "y" : "ies"} skipped (not eligible
                  for ADIF)</summary
                >
                <ul>
                  {#each result.skipped as sk (sk.sequence_no)}<li>#{sk.sequence_no}: {sk.reason}</li>{/each}
                </ul>
              </details>
            {/if}
            {#if result.warnings.length}
              <details>
                <summary>{result.warnings.length} warning(s)</summary>
                <ul>
                  {#each result.warnings as w, i (i)}<li>{w}</li>{/each}
                </ul>
              </details>
            {/if}
          </div>
          <button
            class="small"
            onclick={() =>
              api.openExport(result!.path).catch((e) => app.toast("error", toAppError(e).message))}
            ><ExternalLink size={14} />Open</button
          >
          <button
            class="small"
            onclick={() =>
              api.revealPath(result!.path).catch((e) => app.toast("error", toAppError(e).message))}
            ><FolderSearch size={14} />Reveal</button
          >
        </div>
      {/if}
    </div>
  </section>

  <section class="panel">
    <div class="panel-head">
      <h2><PackagePlus size={20} class="icon" />Portable logbook package (.emlog)</h2>
    </div>
    <div class="panel-body col">
      <p class="muted" style="margin:0">
        A complete copy of this logbook — every entry including voided ones, revision history, templates,
        favorites, logbook settings and attachments — for moving to another computer. Report filters do not
        apply. Your operator profiles and local settings are not included.
      </p>
      <div class="row">
        <button onclick={exportPackage} disabled={pkgBusy}
          >{#if pkgBusy}<LoaderCircle size={16} class="spin" />Exporting…{:else}<PackagePlus
              size={16}
            />Export .emlog package…{/if}</button
        >
        {#if pkgResult}
          <span class="badge ok">VERIFIED</span>
          <span class="mono small"
            >{pkgResult.entry_count} entries · {pkgResult.attachment_count} attachments · SHA-256 {pkgResult.sha256.slice(
              0,
              16,
            )}…</span
          >
          <button
            class="small"
            onclick={() =>
              api.revealPath(pkgResult!.path).catch((e) => app.toast("error", toAppError(e).message))}
            ><FolderSearch size={14} />Reveal</button
          >
        {/if}
      </div>
    </div>
  </section>
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .title {
    display: flex;
    gap: 10px;
    align-items: center;
    font-size: 20px;
  }
  .formats {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: 8px;
  }
  .fmt {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 10px 12px;
    border: 1.5px solid var(--border-strong);
    border-radius: 9px;
    cursor: pointer;
    color: var(--text);
  }
  .fmt.sel {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }
  .fmt:focus-within {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }
  fieldset {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .summary {
    padding: 10px 14px;
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  .small {
    font-size: 12.5px;
  }
</style>
