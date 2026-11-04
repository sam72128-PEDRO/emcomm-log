<script lang="ts">
  import {
    ChartColumn,
    FileDown,
    PackagePlus,
    FileText,
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
  import Form214Dialog from "../components/Form214Dialog.svelte";
  import Form309Dialog from "../components/Form309Dialog.svelte";

  const info = app.active!.info;
  const formats: { id: T.ExportFormat; label: string; desc: string; ext: string; ham?: boolean }[] = [
    { id: "pdf", label: "PDF", desc: "Print-ready report with page numbers", ext: "pdf" },
    { id: "docx", label: "Word (DOCX)", desc: "Editable report document", ext: "docx" },
    { id: "xlsx", label: "Excel (XLSX)", desc: "Spreadsheet with filters and typed UTC times", ext: "xlsx" },
    { id: "csv", label: "CSV", desc: "UTF-8 comma-separated values", ext: "csv" },
    { id: "txt", label: "Text (TXT)", desc: "Readable chronological log", ext: "txt" },
    { id: "adif", label: "ADIF (.adi)", desc: "Qualifying QSOs only, ADIF 3.1.8", ext: "adi", ham: true },
  ];
  const available = formats.filter((f) => !f.ham || info.logbook_type === "ham");
  const presets: { id: T.AdifPreset; label: string; desc: string }[] = [
    {
      id: "general",
      label: "General / QRZ",
      desc: "Every qualifying QSO, for QRZ, LoTW (via TQSL), Club Log and others",
    },
    {
      id: "pota_activator",
      label: "POTA activator",
      desc: "QSOs with your park (My POTA); adds MY_SIG/SIG for POTA's uploader",
    },
    { id: "sota_activator", label: "SOTA activator", desc: "QSOs with your summit (My SOTA)" },
    { id: "sota_chaser", label: "SOTA chaser", desc: "QSOs with a summit worked (SOTA reference)" },
  ];

  let form214 = $state(false);
  let form309 = $state(false);
  let format = $state<T.ExportFormat>("pdf");
  let scope = $state<"all" | "range" | "filter">(app.reportScope);
  let from = $state(todayDateString(app.zone));
  let to = $state(todayDateString(app.zone));
  let includeVoided = $state(app.reportScope === "filter" ? app.logQuery.status !== "active" : false);
  let zone = $state<"local" | "utc">(app.zone);
  let adifPreset = $state<T.AdifPreset>("general");
  let preciseCoords = $state(false);
  let skipAck = $state(false);
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
    adif_preset: adifPreset,
    include_precise_coordinates: preciseCoords,
  });
  const summary = $derived(format === "adif" ? (preview?.adif ?? null) : null);
  const needsAck = $derived(!!summary && summary.skipped.length > 0);

  $effect(() => {
    const r = request;
    void app.dataVersion;
    preview = null;
    previewErr = null;
    skipAck = false;
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
          <label class="check"
            ><input type="checkbox" bind:checked={preciseCoords} /> Include precise coordinates</label
          >
          <span class="hint" style="margin-top:-4px"
            >Off by default: place names and 6-character grids are kept, latitude/longitude are withheld. Turn
            on only when the recipient needs exact positions.{#if format === "adif"}
              For ADIF this writes MY_LAT/MY_LON from the station position only.{/if}</span
          >
          <span class="hint"
            >Every report includes the logbook name and type, generated time and zone, the scope description,
            entry numbers, event and recorded times, and status.</span
          >
        </fieldset>
      </div>
      {#if format === "adif"}
        <fieldset>
          <legend>ADIF preset</legend>
          <div class="formats" role="radiogroup" aria-label="ADIF preset">
            {#each presets as p (p.id)}
              <label class="fmt" class:sel={adifPreset === p.id}>
                <input type="radio" name="adifp" value={p.id} bind:group={adifPreset} class="sr-only" />
                <strong>{p.label}</strong><span class="muted">{p.desc}</span>
              </label>
            {/each}
          </div>
          <span class="hint"
            >One standard ADIF file; the preset only chooses which QSOs qualify. Nothing is uploaded — import
            the file on the service's own website.</span
          >
        </fieldset>
      {/if}
      <div class="summary" aria-live="polite">
        {#if previewErr}<span class="field-error"><CircleAlert size={15} /> {previewErr}</span>
        {:else if preview}
          <strong>{preview.count} record(s)</strong> · {formats.find((f) => f.id === format)?.label}
          {#if preview.adif_eligible !== null}
            · <strong>{preview.adif_eligible}</strong> eligible QSO(s), {preview.adif_skipped} will be skipped with
            reasons{/if}
          <div class="hint">{preview.description}</div>
          {#if summary && summary.eligible > 0}
            <dl class="kv adif-sum">
              {#if summary.station_callsigns.length}<dt>Station callsign</dt>
                <dd class="mono">{summary.station_callsigns.join(", ")}</dd>{/if}
              {#if summary.operators.length}<dt>Operator</dt>
                <dd class="mono">{summary.operators.join(", ")}</dd>{/if}
              {#if summary.refs.length}<dt>References</dt>
                <dd class="mono">{summary.refs.join(", ")}</dd>{/if}
              {#if summary.first_utc}<dt>UTC span</dt>
                <dd class="mono">
                  {summary.first_utc.slice(0, 16).replace("T", " ")} – {(summary.last_utc ?? "")
                    .slice(0, 16)
                    .replace("T", " ")} UTC
                </dd>{/if}
            </dl>
          {/if}
          {#if needsAck && summary}
            <details open={summary.skipped.length <= 10}>
              <summary
                >{summary.skipped.length} entr{summary.skipped.length === 1 ? "y" : "ies"} will be left out</summary
              >
              <ul class="skips">
                {#each summary.skipped as sk (sk.sequence_no)}<li>#{sk.sequence_no}: {sk.reason}</li>{/each}
              </ul>
            </details>
            <label class="check"
              ><input type="checkbox" bind:checked={skipAck} /> Export without these {summary.skipped.length}
              entr{summary.skipped.length === 1 ? "y" : "ies"}</label
            >
          {/if}
        {:else}<span class="muted">Counting…</span>{/if}
      </div>
      <div class="row">
        <button
          class="primary"
          onclick={run}
          disabled={running ||
            !!previewErr ||
            !preview ||
            (needsAck && !skipAck) ||
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

  {#if app.bookType === "emergency"}
    <section class="panel">
      <div class="panel-head"><h2><FileText size={20} class="icon" />ICS forms</h2></div>
      <div class="panel-body col">
        <p class="muted" style="margin:0">
          Filled on FEMA's official forms. Signature lines are left blank for signing after printing, and the
          fields stay editable in any PDF reader.
        </p>
        <div class="row wrap">
          <button onclick={() => (form214 = true)}><FileText size={16} />ICS 214 Activity Log…</button>
          <button onclick={() => (form309 = true)}><FileText size={16} />ICS 309 Communications Log…</button>
          <button onclick={() => app.navigate("log")}><FileText size={16} />ICS 213 General Message…</button>
          <span class="hint"
            >For an ICS 213, select message entries in the Traffic Log (or open one) and choose ICS 213.</span
          >
        </div>
      </div>
    </section>
  {/if}

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

{#if form214}<Form214Dialog onclose={() => (form214 = false)} />{/if}
{#if form309}<Form309Dialog onclose={() => (form309 = false)} />{/if}

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
  .adif-sum {
    margin: 8px 0 4px;
  }
  .skips {
    max-height: 200px;
    overflow: auto;
    margin: 4px 0;
  }
</style>
