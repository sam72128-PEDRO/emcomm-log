<script lang="ts">
  import { onMount } from "svelte";
  import {
    List,
    Search,
    SlidersHorizontal,
    Star,
    Flag,
    Download,
    FileText,
    Printer,
    ChevronLeft,
    ChevronRight,
    Paperclip,
    RotateCcw,
    ArrowRightLeft,
  } from "@lucide/svelte";
  import { app, defaultQuery } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import { modKey } from "../lib/actions";
  import type * as T from "../lib/types";
  import {
    allowedKinds,
    DIRECTION_LABEL,
    DIRECTION_SHORT,
    HANDLING_LABEL,
    KIND_LABEL,
    PRIORITY_LABEL,
  } from "../lib/entryForm";
  import { formatInstant, fromInputValue, todayBoundsUtc, zoneLabel } from "../lib/time";
  import KindBadge from "../components/KindBadge.svelte";
  import Form213Dialog from "../components/Form213Dialog.svelte";
  import Form309Dialog from "../components/Form309Dialog.svelte";

  const book = app.active!;
  const kinds = allowedKinds(book.info.logbook_type);
  let form213 = $state(false);
  let form309 = $state(false);
  let page = $state<T.EntryPage>({ total: 0, items: [] });
  let loading = $state(false);
  let error = $state<string | null>(null);
  let showAdvanced = $state(false);
  let searchEl: HTMLInputElement | undefined = $state();
  let period = $state<"all" | "today" | "24h" | "custom">("all");
  let customFrom = $state("");
  let customTo = $state("");
  let text = $state(app.logQuery.text);
  let station = $state(app.logQuery.station);
  let tagText = $state(app.logQuery.tags.join(", "));
  let debounce: ReturnType<typeof setTimeout> | null = null;

  const q = $derived(app.logQuery);

  async function load() {
    loading = true;
    try {
      page = await api.listEntries(app.logQuery);
      error = null;
    } catch (e) {
      error = toAppError(e).message;
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    JSON.stringify(app.logQuery);
    void app.dataVersion;
    load();
  });

  onMount(() => {
    app.focusSearch = () => {
      searchEl?.focus();
      searchEl?.select();
    };
    return () => {
      app.focusSearch = null;
    };
  });

  // Operators seen in this logbook's shifts (2.1): filter by profile id, or by name for imported shifts.
  const operators = $derived(
    [...new Map(app.sessions.map((x) => [x.profile_id ?? x.operator.name, x.operator.name])).entries()].map(
      ([key, name]) => ({ key, name }),
    ),
  );
  // Shift changes (not the first shift) shown between the entries they fall between, in event-time order.
  const handovers = $derived(
    app.sessions.slice(1).map((x, k) => ({ ...x, prev: app.sessions[k].operator.name })),
  );
  function shiftsBefore(i: number) {
    if (q.sort === "recorded" || q.operator || q.text) return [];
    const t = (iso: string) => Date.parse(iso);
    const cur = t(page.items[i].event_at_utc);
    const prev = i > 0 ? t(page.items[i - 1].event_at_utc) : null;
    return handovers.filter((h) => {
      const at = t(h.started_at_utc);
      if (q.descending) return at >= cur && (prev === null ? q.offset === 0 : at < prev);
      return at <= cur && (prev === null ? q.offset === 0 : at > prev);
    });
  }

  function setQuery(patch: Partial<T.EntryQuery>, resetPage = true) {
    app.logQuery = { ...app.logQuery, ...patch, ...(resetPage ? { offset: 0 } : {}) };
  }

  function onText() {
    if (debounce) clearTimeout(debounce);
    debounce = setTimeout(
      () =>
        setQuery({
          text,
          station,
          tags: tagText
            .split(",")
            .map((t) => t.trim())
            .filter(Boolean),
        }),
      250,
    );
  }

  function applyPeriod() {
    if (period === "all") setQuery({ from_utc: null, to_utc: null });
    else if (period === "today") {
      const [a, b] = todayBoundsUtc(app.zone);
      setQuery({ from_utc: a, to_utc: b });
    } else if (period === "24h")
      setQuery({ from_utc: new Date(Date.now() - 86_400_000).toISOString(), to_utc: null });
    else {
      const a = customFrom ? fromInputValue(`${customFrom}T00:00:00`, app.zone) : null;
      let b: string | null = null;
      if (customTo) {
        const end = fromInputValue(`${customTo}T00:00:00`, app.zone);
        b = end ? new Date(new Date(end).getTime() + 86_400_000).toISOString() : null;
      }
      setQuery({ from_utc: a, to_utc: b });
    }
  }

  function toggleIn<K extends "kinds" | "directions" | "priorities" | "handling">(
    key: K,
    v: T.EntryQuery[K][number],
  ) {
    const cur = app.logQuery[key] as string[];
    const next = cur.includes(v as string) ? cur.filter((x) => x !== v) : [...cur, v as string];
    setQuery({ [key]: next } as Partial<T.EntryQuery>);
  }

  function reset() {
    text = "";
    station = "";
    tagText = "";
    period = "all";
    app.logQuery = defaultQuery();
    app.selection = [];
  }

  const activeFilters = $derived(
    [q.text, q.station, q.from_utc, q.to_utc].filter(Boolean).length +
      q.kinds.length +
      q.directions.length +
      q.priorities.length +
      q.handling.length +
      q.tags.length +
      (q.status !== "active" ? 1 : 0) +
      (q.favorites_only ? 1 : 0) +
      (q.flagged_only ? 1 : 0),
  );

  function isSelected(id: string) {
    return app.selection.includes(id);
  }
  function toggleSel(id: string) {
    app.selection = isSelected(id) ? app.selection.filter((x) => x !== id) : [...app.selection, id];
  }
  const allOnPageSelected = $derived(
    page.items.length > 0 && page.items.every((e) => app.selection.includes(e.id)),
  );
  function toggleAll() {
    const ids = page.items.map((e) => e.id);
    app.selection = allOnPageSelected
      ? app.selection.filter((x) => !ids.includes(x))
      : [...new Set([...app.selection, ...ids])];
  }

  async function toggleFavorite(e: T.Entry) {
    try {
      await api.setFavorite(e.id, !e.favorite);
      app.bumpData();
    } catch (err) {
      app.toast("error", toAppError(err).message);
    }
  }

  function rowKey(ev: KeyboardEvent, e: T.Entry, i: number) {
    if (ev.key === "Enter") {
      ev.preventDefault();
      app.detailEntryId = e.id;
    } else if (ev.key === " ") {
      ev.preventDefault();
      toggleSel(e.id);
    } else if (ev.key === "ArrowDown" || ev.key === "ArrowUp") {
      ev.preventDefault();
      const rows = document.querySelectorAll<HTMLElement>("tr[data-row]");
      rows[Math.max(0, Math.min(rows.length - 1, i + (ev.key === "ArrowDown" ? 1 : -1)))]?.focus();
    }
  }

  async function exportCurrent(selection: boolean) {
    app.reportScope = "filter";
    if (selection) app.logQuery = { ...app.logQuery, ids: [...app.selection] };
    else if (app.logQuery.ids.length) app.logQuery = { ...app.logQuery, ids: [] };
    await app.navigate("reports");
  }

  async function print() {
    const dest = await api.pickSaveFile("Save printable PDF", `${book.info.name} - Traffic Log.pdf`, [
      { name: "PDF", extensions: ["pdf"] },
    ]);
    if (!dest) return;
    try {
      const r = await api.runExport(
        {
          format: "pdf",
          scope: "filter",
          range_from: null,
          range_to: null,
          filter: { ...app.logQuery, limit: 0, offset: 0 },
          include_voided: app.logQuery.status !== "active",
          display_zone: app.zone,
        },
        dest,
      );
      app.toast("ok", `PDF with ${r.exported} entries created; opening for printing.`);
      await api.openExport(r.path);
    } catch (e) {
      app.toast("error", `Print failed: ${toAppError(e).message}`);
    }
  }

  const start = $derived(page.total === 0 ? 0 : q.offset + 1);
  const end = $derived(Math.min(q.offset + q.limit, page.total));
  const pages = $derived(Math.max(1, Math.ceil(page.total / q.limit)));
  const curPage = $derived(Math.floor(q.offset / q.limit) + 1);
</script>

<section class="panel log">
  <div class="panel-head">
    <h1 class="title"><List size={22} class="icon" />Traffic Log</h1>
    <span class="spacer"></span>
    <span class="muted">Times in {zoneLabel(app.zone)}</span>
  </div>
  <div class="filters">
    <div class="search">
      <Search size={16} aria-hidden="true" />
      <input
        bind:this={searchEl}
        type="search"
        placeholder="Search traffic (station, text, etc.) — {modKey()}F"
        aria-label="Search entries"
        bind:value={text}
        oninput={onText}
      />
    </div>
    <select
      aria-label="Filter by type"
      value={q.kinds.length === 1 ? q.kinds[0] : ""}
      onchange={(e) =>
        setQuery({ kinds: e.currentTarget.value ? [e.currentTarget.value as T.EntryKind] : [] })}
    >
      <option value="">All Types</option>
      {#each kinds as k (k)}<option value={k}>{KIND_LABEL[k]}</option>{/each}
    </select>
    <input
      type="text"
      aria-label="Filter by station, callsign, from or to"
      placeholder="All Stations"
      bind:value={station}
      oninput={onText}
      style="max-width:170px"
    />
    <select aria-label="Time period" bind:value={period} onchange={applyPeriod}>
      <option value="all">All time</option>
      <option value="today">Today</option>
      <option value="24h">Last 24 hours</option>
      <option value="custom">Date range…</option>
    </select>
    <select
      aria-label="Status"
      value={q.status}
      onchange={(e) => setQuery({ status: e.currentTarget.value as T.EntryQuery["status"] })}
    >
      <option value="active">Active entries</option>
      <option value="void">Voided only</option>
      <option value="all">Active + voided</option>
    </select>
    {#if operators.length > 1 || q.operator}
      <select
        aria-label="Operator"
        value={q.operator ?? ""}
        onchange={(e) => setQuery({ operator: e.currentTarget.value })}
        style="max-width:170px"
      >
        <option value="">All operators</option>
        {#each operators as o (o.key)}<option value={o.key}>{o.name}</option>{/each}
      </select>
    {/if}
    <button
      class="icon-only"
      aria-expanded={showAdvanced}
      aria-controls="adv"
      title="More filters"
      aria-label="More filters ({activeFilters} active)"
      onclick={() => (showAdvanced = !showAdvanced)}
    >
      <SlidersHorizontal size={17} />{#if activeFilters}<span class="dotcount">{activeFilters}</span>{/if}
    </button>
    {#if activeFilters}<button class="small ghost" onclick={reset}><RotateCcw size={14} />Reset</button>{/if}
  </div>
  {#if period === "custom"}
    <div class="filters">
      <label for="tl-from">From</label><input
        id="tl-from"
        type="date"
        bind:value={customFrom}
        onchange={applyPeriod}
        style="max-width:170px"
      />
      <label for="tl-to">To (inclusive)</label><input
        id="tl-to"
        type="date"
        bind:value={customTo}
        onchange={applyPeriod}
        style="max-width:170px"
      />
      <span class="hint">Dates in {zoneLabel(app.zone)}</span>
    </div>
  {/if}
  {#if showAdvanced}
    <div class="advanced" id="adv">
      <fieldset>
        <legend>Type</legend>{#each kinds as k (k)}<label class="check"
            ><input
              type="checkbox"
              checked={q.kinds.includes(k)}
              onchange={() => toggleIn("kinds", k)}
            />{KIND_LABEL[k]}</label
          >{/each}
      </fieldset>
      <fieldset>
        <legend>Direction</legend>{#each Object.entries(DIRECTION_LABEL) as [v, l] (v)}<label class="check"
            ><input
              type="checkbox"
              checked={q.directions.includes(v as T.Direction)}
              onchange={() => toggleIn("directions", v as T.Direction)}
            />{l}</label
          >{/each}
      </fieldset>
      <fieldset>
        <legend>Priority</legend>{#each Object.entries(PRIORITY_LABEL) as [v, l] (v)}<label class="check"
            ><input
              type="checkbox"
              checked={q.priorities.includes(v as T.Priority)}
              onchange={() => toggleIn("priorities", v as T.Priority)}
            />{l}</label
          >{/each}
      </fieldset>
      <fieldset>
        <legend>Handling</legend>{#each Object.entries(HANDLING_LABEL) as [v, l] (v)}<label class="check"
            ><input
              type="checkbox"
              checked={q.handling.includes(v as T.Handling)}
              onchange={() => toggleIn("handling", v as T.Handling)}
            />{l}</label
          >{/each}
      </fieldset>
      <fieldset>
        <legend>More</legend>
        <label class="check"
          ><input
            type="checkbox"
            checked={q.favorites_only}
            onchange={(e) => setQuery({ favorites_only: e.currentTarget.checked })}
          />Favorites only</label
        >
        <label class="check"
          ><input
            type="checkbox"
            checked={q.flagged_only}
            onchange={(e) => setQuery({ flagged_only: e.currentTarget.checked })}
          />Flagged only</label
        >
        <div class="field">
          <label for="tl-tags">Tags (comma separated)</label><input
            id="tl-tags"
            type="text"
            bind:value={tagText}
            oninput={onText}
          />
        </div>
        <div class="field">
          <label for="tl-sort">Sort by</label>
          <select
            id="tl-sort"
            value="{q.sort}:{q.descending}"
            onchange={(e) => {
              const [s, d] = e.currentTarget.value.split(":");
              setQuery({ sort: s as "event" | "recorded", descending: d === "true" });
            }}
          >
            <option value="event:true">Event time, newest first</option>
            <option value="event:false">Event time, oldest first</option>
            <option value="recorded:true">Recorded time, newest first</option>
            <option value="recorded:false">Recorded time, oldest first</option>
          </select>
        </div>
      </fieldset>
    </div>
  {/if}
  {#if error}<div class="banner error" role="alert" style="margin:0 16px">{error}</div>{/if}
  {#if q.status !== "active"}<div class="banner info" style="margin:8px 16px 0">
      Showing voided entries ({q.status === "void" ? "only voided" : "active and voided"}).
    </div>{/if}

  <div class="tablewrap" aria-busy={loading}>
    <table class="data" aria-label="Log entries" aria-rowcount={page.total}>
      <thead>
        <tr>
          <th scope="col" style="width:34px"
            ><input
              type="checkbox"
              aria-label="Select all on this page"
              checked={allOnPageSelected}
              onchange={toggleAll}
            /></th
          >
          <th scope="col">ID</th>
          <th scope="col">{q.sort === "recorded" ? "Recorded" : "Date / Time"}</th>
          <th scope="col">Station</th>
          <th scope="col">Type</th>
          <th scope="col">Priority</th>
          <th scope="col">Summary</th>
          <th scope="col">Status</th>
          <th scope="col"><span class="sr-only">Favorite</span></th>
        </tr>
      </thead>
      <tbody>
        {#each page.items as e, i (e.id)}
          {#each shiftsBefore(i) as sh (sh.id)}
            <tr class="shift" aria-label="Shift change">
              <td colspan="9"
                ><ArrowRightLeft size={13} /> Shift change {formatInstant(
                  sh.started_at_utc,
                  app.zone,
                  app.clock,
                )}:
                <strong>{sh.operator.name}</strong> on duty{sh.prev ? `, relieving ${sh.prev}` : ""}{sh.note
                  ? ` — ${sh.note}`
                  : ""}</td
              >
            </tr>
          {/each}
          <tr
            data-row
            class="clickable"
            class:void={e.status === "void"}
            tabindex="0"
            aria-selected={isSelected(e.id)}
            onclick={() => (app.detailEntryId = e.id)}
            onkeydown={(ev) => rowKey(ev, e, i)}
          >
            <td
              ><input
                type="checkbox"
                aria-label="Select entry {e.sequence_no}"
                checked={isSelected(e.id)}
                onclick={(ev) => ev.stopPropagation()}
                onchange={() => toggleSel(e.id)}
                tabindex="-1"
              /></td
            >
            <td class="mono">{String(e.sequence_no).padStart(4, "0")}</td>
            <td class="nowrap"
              >{formatInstant(
                q.sort === "recorded" ? e.recorded_at_utc : e.event_at_utc,
                app.zone,
                app.clock,
              )}</td
            >
            <td class="nowrap"
              >{e.ham?.their_call || e.from_text || "—"}{#if e.to_text}{` → ${e.to_text}`}{/if}</td
            >
            <td class="nowrap"
              ><KindBadge kind={e.kind} /> <span class="muted small">{DIRECTION_SHORT[e.direction]}</span></td
            >
            <td
              >{#if e.priority === "routine"}Normal{:else}<span
                  class="badge {e.priority === 'emergency' ? 'danger' : 'warn'}"
                  >{PRIORITY_LABEL[e.priority]}</span
                >{/if}</td
            >
            <td class="summary"
              ><div class="sumwrap">
                <span class="ellipsis">{e.subject}</span>
                {#if e.attachments.length}<Paperclip
                    size={13}
                    aria-label="{e.attachments.length} attachment(s)"
                  />{/if}
                {#if e.current_revision > 1}<span class="badge neutral" title="Edited"
                    >rev {e.current_revision}</span
                  >{/if}
              </div></td
            >
            <td class="nowrap">
              {#if e.status === "void"}<span class="badge danger">VOID</span>
              {:else if e.handling}<span
                  class="badge {e.handling === 'pending' || e.handling === 'awaiting_ack'
                    ? 'warn'
                    : e.handling === 'sent'
                      ? 'info'
                      : 'ok'}">{HANDLING_LABEL[e.handling]}</span
                >
              {:else}<span class="badge ok">Logged</span>{/if}
              {#if e.flagged}<Flag size={14} class="flag" aria-label="Flagged for report" />{/if}
            </td>
            <td>
              <button
                class="ghost icon-only small"
                aria-pressed={e.favorite}
                aria-label="{e.favorite ? 'Remove' : 'Add'} favorite for entry {e.sequence_no}"
                onclick={(ev) => {
                  ev.stopPropagation();
                  toggleFavorite(e);
                }}
                tabindex="-1"
                disabled={app.readOnly}
              >
                <Star size={16} fill={e.favorite ? "currentColor" : "none"} class={e.favorite ? "fav" : ""} />
              </button>
            </td>
          </tr>
        {:else}
          <tr
            ><td colspan="9" class="empty"
              >{loading
                ? "Loading…"
                : activeFilters
                  ? "No entries match these filters."
                  : "No entries yet. Press " + modKey() + "N to add one."}</td
            ></tr
          >
        {/each}
      </tbody>
    </table>
  </div>
  <footer class="foot">
    <span role="status"
      >Showing {start}–{end} of {page.total} entries{#if app.selection.length}{` · ${app.selection.length} selected`}{/if}</span
    >
    <span class="spacer"></span>
    <button
      class="icon-only"
      aria-label="Previous page"
      disabled={curPage <= 1}
      onclick={() => setQuery({ offset: Math.max(0, q.offset - q.limit) }, false)}
      ><ChevronLeft size={17} /></button
    >
    <span class="pageno" aria-label="Page {curPage} of {pages}">{curPage} / {pages}</span>
    <button
      class="icon-only"
      aria-label="Next page"
      disabled={curPage >= pages}
      onclick={() => setQuery({ offset: q.offset + q.limit }, false)}><ChevronRight size={17} /></button
    >
    <span class="spacer"></span>
    {#if app.selection.length}<button onclick={() => exportCurrent(true)}
        ><Download size={16} />Export Selection ({app.selection.length})</button
      >{/if}
    {#if app.bookType === "emergency"}<button
        onclick={() => (form213 = true)}
        disabled={!app.selection.length}
        title={app.selection.length
          ? "Fill an ICS 213 for each selected message"
          : "Select one or more message entries first"}
        ><FileText size={16} />ICS 213{app.selection.length ? ` (${app.selection.length})` : ""}…</button
      ><button
        onclick={() => (form309 = true)}
        title={app.selection.length
          ? "Communications log of the selected entries"
          : "Communications log of an operational period"}><FileText size={16} />ICS 309…</button
      >{/if}
    <button onclick={() => exportCurrent(false)}><Download size={16} />Export…</button>
    <button onclick={print}><Printer size={16} />Print</button>
  </footer>
</section>

{#if form213}<Form213Dialog entryIds={[...app.selection]} onclose={() => (form213 = false)} />{/if}
{#if form309}<Form309Dialog
    entryIds={[...app.selection]}
    opId={page.items.find((e) => app.selection.includes(e.id) && e.op_id)?.op_id ?? null}
    onclose={() => (form309 = false)}
  />{/if}

<style>
  .shift td {
    font-size: 12.5px;
    color: var(--muted);
    background: color-mix(in srgb, var(--cyan) 6%, transparent);
    padding: 5px 12px;
  }
  .log {
    display: flex;
    flex-direction: column;
    min-height: calc(100vh - 100px);
  }
  .title {
    display: flex;
    gap: 10px;
    align-items: center;
    font-size: 20px;
  }
  .filters {
    display: flex;
    gap: 8px;
    padding: 12px 16px 0;
    align-items: center;
    flex-wrap: wrap;
  }
  .filters select {
    width: auto;
  }
  .search {
    flex: 1;
    min-width: 260px;
    display: flex;
    align-items: center;
    gap: 6px;
    border: 1px solid var(--border-strong);
    border-radius: 7px;
    padding: 0 8px;
    background: var(--input-bg);
  }
  .search input {
    border: none;
    outline: none;
    background: transparent;
  }
  .search:focus-within {
    border-color: var(--accent);
  }
  .advanced {
    display: grid;
    grid-template-columns: repeat(5, minmax(0, 1fr));
    gap: 10px;
    padding: 12px 16px 0;
  }
  .advanced fieldset {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .tablewrap {
    flex: 1;
    overflow: auto;
    margin: 12px 16px;
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  .summary {
    max-width: 460px;
  }
  .sumwrap {
    display: flex;
    gap: 6px;
    align-items: center;
    min-width: 0;
  }
  .summary .ellipsis {
    flex: 1;
    min-width: 0;
  }
  .small {
    font-size: 12px;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 16px 14px;
    flex-wrap: wrap;
  }
  .pageno {
    min-width: 54px;
    text-align: center;
  }
  .dotcount {
    background: var(--fill);
    color: #fff;
    font-size: 11px;
    border-radius: 999px;
    padding: 0 5px;
  }
  :global(.fav) {
    color: var(--warn);
  }
  :global(.flag) {
    color: var(--danger);
    vertical-align: middle;
    margin-left: 4px;
  }
</style>
