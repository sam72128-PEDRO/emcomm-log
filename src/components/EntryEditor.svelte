<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import {
    CircleAlert,
    CircleCheck,
    Paperclip,
    X,
    Save,
    Eraser,
    LoaderCircle,
    FileClock,
    RefreshCw,
    BookmarkPlus,
  } from "@lucide/svelte";
  import { app } from "../lib/state.svelte";
  import { api, newId, toAppError } from "../lib/api";
  import { confirm } from "../lib/confirm.svelte";
  import { modKey } from "../lib/actions";
  import type * as T from "../lib/types";
  import {
    allowedKinds,
    applyTemplate,
    bandForHz,
    BANDS,
    DIRECTION_LABEL,
    formFromEntry,
    formHasContent,
    HANDLING_LABEL,
    KIND_LABEL,
    MODES,
    newForm,
    parseFrequencyHz,
    PRIORITY_LABEL,
    toEntryInput,
    toTemplateDefaults,
    validateForm,
    carrySession,
  } from "../lib/entryForm";
  import type { EntryForm, FieldErrors, FormContext } from "../lib/entryForm";
  import { formatInstant } from "../lib/time";
  import { currentPeriod } from "../lib/periods";
  import { channelHint, channelValue } from "../lib/plan";
  import DateTimeInput from "./DateTimeInput.svelte";
  import LocationEditor from "./LocationEditor.svelte";
  import TagInput from "./TagInput.svelte";

  let {
    mode,
    entry = null,
    initial = null,
    initialDraft = null,
    templates = [],
    onsaved,
    oncancel,
    ontemplatesaved,
    compact = false,
  }: {
    mode: "new" | "edit";
    entry?: T.Entry | null;
    initial?: EntryForm | null;
    initialDraft?: T.Draft | null;
    templates?: T.EntryTemplate[];
    onsaved?: (e: T.Entry) => void;
    oncancel?: () => void;
    ontemplatesaved?: () => void;
    compact?: boolean;
  } = $props();

  const book = app.active!;
  const type = book.info.logbook_type;
  const preset = book.settings.preset;
  const kinds = allowedKinds(type);
  const hidden = new Set(preset.hidden_fields);
  const lbl = (field: string, fallback: string) => preset.field_labels[field] ?? fallback;
  const ctx = (): FormContext => ({
    type,
    preset,
    profile: app.profile,
    incident: app.active?.info.incident ?? book.info.incident,
    station: app.station,
    currentOpId: type === "emergency" ? (currentPeriod(app.periods)?.id ?? null) : null,
  });

  function startForm(): EntryForm {
    if (initialDraft) return (initialDraft.content as { form: EntryForm }).form;
    if (initial) return initial;
    if (mode === "edit" && entry) return formFromEntry(entry);
    return newForm(ctx());
  }

  let form = $state<EntryForm>(startForm());
  // The editor is remounted for each new target, so capturing the initial props is intended.
  let baseline = $state<EntryForm>(
    untrack(() => (mode === "edit" && entry ? formFromEntry(entry) : newForm(ctx()))),
  );
  // Periods load after the logbook opens; a new, untouched entry picks up the current one then
  // (once, so a deliberate "None" is not overridden later).
  let opDefaulted = false;
  $effect(() => {
    const id = type === "emergency" ? (currentPeriod(app.periods)?.id ?? "") : "";
    untrack(() => {
      if (opDefaulted || !id) return;
      opDefaulted = true;
      if (mode === "new" && !form.op_id && !baseline.op_id) {
        form.op_id = id;
        baseline.op_id = id;
      }
    });
  });
  // Channels from the entry's period plan (ICS 205), offered first in the Channel field.
  let planChannels = $state<T.ChannelRow[]>([]);
  $effect(() => {
    const id = form.op_id;
    void app.plans;
    if (!id) {
      planChannels = [];
      return;
    }
    app.loadPlan(id).then((p) => {
      if (form.op_id === id) planChannels = (p?.rows ?? []).filter((r) => channelValue(r));
    });
  });
  const periodChoices = $derived(app.periods.filter((p) => p.status === "active" || p.id === form.op_id));
  let errors = $state<FieldErrors>({});
  let saving = $state(false);
  let saveError = $state<T.AppError | null>(null);
  let addAnother = $state(true);
  let reason = $state("");
  let draftId = untrack(() => initialDraft?.draft_id) ?? newId();
  let pending: { id: string; hash: string } | null = null;
  let draftTimer: ReturnType<typeof setTimeout> | null = null;
  let draftDirty = false;
  let root: HTMLDivElement | undefined = $state();
  const fid = (s: string) => `ed-${mode}-${s}`;

  const dirty = $derived(formHasContent(form, baseline) || form.attachments.length > 0);
  $effect(() => {
    void dirty;
    app.updateDirty();
  });

  const freq = $derived(parseFrequencyHz(form.freqText, form.freqUnit));

  // Local suggestions from this logbook's recent history (2.1 §5.3). Offered through datalists only:
  // nothing is filled in until the operator picks a value.
  let recents = $state<T.Recents>({});
  $effect(() => {
    void app.dataVersion;
    const fields: T.RecentField[] =
      type === "ham"
        ? ["their_call", "frequency_hz", "mode", "pota_ref", "sota_ref", "channel"]
        : ["station", "channel"];
    api
      .recentValues(fields)
      .then((r) => (recents = r && typeof r === "object" ? r : {}))
      .catch(() => (recents = {}));
  });
  const recentFreqs = $derived(
    (recents.frequency_hz ?? [])
      .map(Number)
      .filter((hz) => hz > 0)
      .map((hz) =>
        form.freqUnit === "kHz"
          ? String(hz / 1e3)
          : form.freqUnit === "GHz"
            ? String(hz / 1e9)
            : String(hz / 1e6),
      ),
  );
  const suggestedBand = $derived(freq.hz ? bandForHz(freq.hz) : null);
  const blocked = $derived(!!app.storageBlocked || app.readOnly);

  // ---------- drafts ----------
  function draftMeta(): T.DraftMeta {
    return {
      draft_id: draftId,
      logbook_id: book.info.id,
      logbook_name: book.info.name,
      editor: mode,
      entry_id: entry?.id ?? null,
      entry_sequence: entry?.sequence_no ?? null,
      base_revision: entry?.current_revision ?? null,
    };
  }

  async function writeDraft() {
    if (draftTimer) clearTimeout(draftTimer);
    draftTimer = null;
    if (!draftDirty) return;
    draftDirty = false;
    if (!dirty) {
      await api.deleteDraft(book.info.id, draftId).catch(() => {});
      app.draftStatus = { kind: "idle" };
      return;
    }
    app.draftStatus = { kind: "pending" };
    try {
      const at = await api.saveDraft(draftMeta(), { form: $state.snapshot(form), mode });
      app.draftStatus = { kind: "saved", at };
    } catch (e) {
      app.draftStatus = { kind: "failed", error: toAppError(e) };
      app.announce(`Draft Save Failed: ${toAppError(e).message}. Your text is still in the form.`, true);
    }
  }

  function changed() {
    draftDirty = true;
    if (saveError && !app.storageBlocked) saveError = null;
    if (draftTimer) clearTimeout(draftTimer);
    draftTimer = setTimeout(writeDraft, 2000);
  }

  $effect(() => {
    // Any edit to the form schedules a draft write ~2 s later.
    JSON.stringify(form);
    if (mounted) changed();
  });
  let mounted = false;

  onMount(() => {
    const unregister = app.registerEditor({
      save: () => save(),
      flush: async () => {
        draftDirty = draftDirty || dirty;
        await writeDraft();
      },
      dirty: () => dirty,
      root: () => root,
    });
    const onUnload = () => {
      if (dirty) void writeDraft();
    };
    window.addEventListener("beforeunload", onUnload);
    tick().then(() => {
      mounted = true;
      focusFirst();
    });
    return () => {
      window.removeEventListener("beforeunload", onUnload);
      if (draftTimer) {
        clearTimeout(draftTimer);
        void writeDraft();
      }
      unregister();
    };
  });

  function focusFirst() {
    // Never pull focus out from under an open dialog that this editor is not part of.
    const dialogs = document.querySelectorAll("[role=dialog]");
    const top = dialogs[dialogs.length - 1];
    if (top && !top.contains(root ?? null)) return;
    const target = root?.querySelector<HTMLElement>(
      mode === "edit" ? `#${fid("subject")}` : `#${fid("from")}, #${fid("subject")}`,
    );
    target?.focus();
  }

  // ---------- save ----------
  async function save() {
    if (saving) return;
    if (app.readOnly) {
      app.announce("This logbook is archived and read-only", true);
      return;
    }
    if (app.storageBlocked) {
      app.announce(`Save blocked: ${app.storageBlocked.message}`, true);
      return;
    }
    errors = validateForm(form, type);
    const firstErr = Object.keys(errors)[0];
    if (firstErr) {
      app.announce(`Cannot save: ${errors[firstErr]}`, true);
      await tick();
      root?.querySelector<HTMLElement>("[aria-invalid='true']")?.focus();
      return;
    }
    const input = toEntryInput($state.snapshot(form) as EntryForm, type, app.zone);
    const hash = JSON.stringify(input) + reason;
    if (!pending || pending.hash !== hash) pending = { id: newId(), hash };
    saving = true;
    saveError = null;
    app.saveStatus = { kind: "saving" };
    try {
      const res =
        mode === "edit" && entry
          ? await api.saveChanges(entry.id, entry.current_revision, input, reason, pending.id, draftId)
          : await api.saveEntry(input, pending.id, draftId);
      pending = null;
      if (draftTimer) clearTimeout(draftTimer);
      draftDirty = false;
      app.draftStatus = { kind: "idle" };
      if (res.unchanged) {
        app.announce("No changes to save");
        app.toast("info", "No changes — nothing was written.");
      } else {
        app.saveStatus = {
          kind: "saved",
          seq: res.entry.sequence_no,
          at: res.entry.updated_at_utc,
          edit: mode === "edit",
          duplicate: res.duplicate,
        };
        app.announce(
          `${mode === "edit" ? "Changes Saved" : "Entry Saved"}: number ${res.entry.sequence_no}, revision ${res.entry.current_revision}`,
        );
      }
      app.bumpData();
      // The first save in a logbook starts its first shift; show it.
      if (!app.shift) void app.refreshIdentity();
      if (mode === "new") {
        draftId = newId();
        const carried = carrySession($state.snapshot(form) as EntryForm, newForm(ctx()));
        form = carried;
        baseline = structuredClone(carried);
        errors = {};
        await tick();
        focusFirst();
      }
      onsaved?.(res.entry);
    } catch (e) {
      const err = toAppError(e);
      saveError = err;
      if (err.field) errors = { ...errors, [err.field]: err.message };
      app.noteSaveFailure(err);
      draftDirty = true;
      await writeDraft();
    } finally {
      saving = false;
    }
  }

  async function clearForm() {
    if (dirty) {
      const a = await confirm.ask({
        title: "Clear the form?",
        message: "Everything typed in this entry will be discarded, including its recovery draft.",
        buttons: [
          { id: "clear", label: "Clear", tone: "danger" },
          { id: "cancel", label: "Keep editing", tone: "primary" },
        ],
        cancelId: "cancel",
      });
      if (a.id !== "clear") return;
    }
    for (const a of form.attachments) api.discardStaged(a.staging_id).catch(() => {});
    await api.deleteDraft(book.info.id, draftId).catch(() => {});
    draftId = newId();
    form = newForm(ctx());
    baseline = newForm(ctx());
    errors = {};
    saveError = null;
    pending = null;
    app.draftStatus = { kind: "idle" };
    focusFirst();
  }

  async function cancelEdit() {
    if (dirty) {
      const a = await confirm.ask({
        title: "Discard changes?",
        message: `Your unsaved changes to entry #${entry?.sequence_no} will be discarded.`,
        buttons: [
          { id: "discard", label: "Discard changes", tone: "danger" },
          { id: "cancel", label: "Keep editing", tone: "primary" },
        ],
        cancelId: "cancel",
      });
      if (a.id !== "discard") return;
    }
    if (draftTimer) clearTimeout(draftTimer);
    draftDirty = false;
    await api.deleteDraft(book.info.id, draftId).catch(() => {});
    app.draftStatus = { kind: "idle" };
    oncancel?.();
  }

  async function attach() {
    try {
      const s = await api.stageAttachment();
      if (s) {
        form.attachments = [...form.attachments, s];
        app.announce(`Attached ${s.display_name}`);
      }
    } catch (e) {
      app.toast("error", `Attachment failed: ${toAppError(e).message}`);
    }
  }

  function removeAttachment(a: T.StagedAttachment) {
    form.attachments = form.attachments.filter((x) => x.staging_id !== a.staging_id);
    api.discardStaged(a.staging_id).catch(() => {});
  }

  function useTemplate(t: T.EntryTemplate) {
    form = applyTemplate(newForm(ctx()), t.defaults, kinds);
    app.announce(`Template ${t.name} applied`);
    tick().then(() => root?.querySelector<HTMLElement>(`#${fid("subject")}`)?.focus());
  }

  async function saveAsTemplate() {
    const a = await confirm.ask({
      title: "Save as template",
      message: "Saves the current field values (without any times) as a reusable template in this logbook.",
      input: { label: "Template name", value: form.subject.slice(0, 80), maxLength: 80 },
      buttons: [
        { id: "ok", label: "Save template", tone: "primary" },
        { id: "cancel", label: "Cancel" },
      ],
      cancelId: "cancel",
    });
    if (a.id !== "ok" || !a.value.trim()) return;
    try {
      await api.saveTemplate(
        null,
        a.value.trim(),
        toTemplateDefaults($state.snapshot(form) as EntryForm, type),
      );
      app.toast("ok", `Template "${a.value.trim()}" saved`);
      ontemplatesaved?.();
    } catch (e) {
      app.toast("error", toAppError(e).message);
    }
  }

  function onKindChange() {
    if (type === "emergency")
      form.em.record_type =
        form.kind === "message" ? "message" : form.kind === "activity" ? "activity" : "general";
  }

  const err = (k: string) => errors[k] ?? "";
</script>

<div
  class="editor"
  class:compact
  bind:this={root}
  role="form"
  aria-label={mode === "edit" ? `Edit entry ${entry?.sequence_no}` : "New log entry"}
>
  {#if app.storageBlocked}
    <div class="banner error" role="alert">
      <CircleAlert size={18} class="icon" />
      <div class="grow">
        <strong>Live logbook location unavailable — final saves are blocked.</strong>
        {app.storageBlocked.message} Your text stays here and in a local recovery draft.
        <div class="row" style="margin-top:8px">
          <button class="small" onclick={() => app.recheckStorage()}
            ><RefreshCw size={14} />Recheck storage</button
          >
          <button class="small" onclick={() => (app.dialog = "data")}>Recovery options…</button>
        </div>
      </div>
    </div>
  {/if}
  {#if app.readOnly}
    <div class="banner warn">
      This logbook is archived and read-only. Unarchive it from Logbooks to add or change entries.
    </div>
  {/if}

  <div class="grid-4">
    <DateTimeInput
      id={fid("time")}
      label="Date / Time"
      bind:value={form.event_at_utc}
      zone={app.zone}
      required
      error={err("event_at_utc")}
    />
    <div class="field">
      <label for={fid("kind")}>Type<span class="req">*</span></label>
      <select id={fid("kind")} bind:value={form.kind} onchange={onKindChange}>
        {#each kinds as k (k)}<option value={k}>{KIND_LABEL[k]}</option>{/each}
      </select>
    </div>
    <div class="field">
      <label for={fid("dir")}>Direction</label>
      <select id={fid("dir")} bind:value={form.direction}>
        {#each Object.entries(DIRECTION_LABEL) as [v, l] (v)}<option value={v}>{l}</option>{/each}
      </select>
    </div>
    <div class="field">
      <label for={fid("pri")}>Priority</label>
      <select id={fid("pri")} bind:value={form.priority}>
        {#each Object.entries(PRIORITY_LABEL) as [v, l] (v)}<option value={v}>{l}</option>{/each}
      </select>
    </div>
    {#if !hidden.has("from_text")}
      <div class="field">
        <label for={fid("from")}>{lbl("from_text", "From / Station")}</label>
        <input
          id={fid("from")}
          type="text"
          bind:value={form.from_text}
          maxlength="120"
          autocomplete="off"
          list={fid("stations")}
        />
      </div>
    {/if}
    {#if !hidden.has("to_text")}
      <div class="field">
        <label for={fid("to")}>{lbl("to_text", "To")}</label>
        <input
          id={fid("to")}
          type="text"
          bind:value={form.to_text}
          maxlength="120"
          autocomplete="off"
          list={fid("stations")}
        />
      </div>
    {/if}
    {#if !hidden.has("channel")}
      <div class="field">
        <label for={fid("channel")}>{lbl("channel", "Channel / Frequency")}</label>
        <input
          id={fid("channel")}
          type="text"
          bind:value={form.channel}
          maxlength="120"
          list={fid("channels")}
          autocomplete="off"
        />
        <datalist id={fid("channels")}
          >{#each planChannels as r, i (i)}<option value={channelValue(r)}>{channelHint(r)}</option
            >{/each}{#each preset.channels as c (c)}<option value={c}
            ></option>{/each}{#each (recents.channel ?? []).filter((c) => !preset.channels.includes(c)) as c (c)}<option
              value={c}>recent</option
            >{/each}</datalist
        >
        {#if planChannels.length}<span class="hint"
            >{planChannels.length} channel{planChannels.length === 1 ? "" : "s"} in this period's plan</span
          >{/if}
      </div>
    {/if}
    {#if !hidden.has("handling") && !compact}
      <div class="field">
        <label for={fid("handling")}>Message handling</label>
        <select id={fid("handling")} bind:value={form.handling}>
          <option value="">Not tracked</option>
          {#each Object.entries(HANDLING_LABEL) as [v, l] (v)}<option value={v}>{l}</option>{/each}
        </select>
      </div>
    {/if}
  </div>

  <div class="field">
    <label for={fid("subject")}>{lbl("subject", "Summary")}<span class="req">*</span></label>
    <input
      id={fid("subject")}
      type="text"
      bind:value={form.subject}
      maxlength="300"
      required
      aria-invalid={err("subject") ? "true" : undefined}
      aria-describedby={err("subject") ? fid("subject-err") : undefined}
    />
    {#if err("subject")}<span class="field-error" id={fid("subject-err")}>{err("subject")}</span>{/if}
  </div>
  <div class="field">
    <label for={fid("body")}
      >{lbl("body", form.kind === "activity" ? "Activity narrative" : "Message / Details")}</label
    >
    <textarea id={fid("body")} bind:value={form.body} rows={compact ? 3 : 5} maxlength="100000"></textarea>
    <span class="hint">{form.body.length.toLocaleString()} / 100,000</span>
  </div>

  {#if type === "ham" && form.kind === "qso" && compact}
    <div class="field">
      <label for={fid("call")}>Their callsign<span class="req">*</span></label>
      <input
        id={fid("call")}
        type="text"
        bind:value={form.ham.their_call}
        maxlength="20"
        list={fid("calls")}
        autocomplete="off"
        class="mono"
        aria-invalid={err("ham.their_call") ? "true" : undefined}
      />
      {#if err("ham.their_call")}<span class="field-error">{err("ham.their_call")}</span>{/if}
    </div>
  {/if}
  {#if type === "ham" && form.kind === "qso" && !compact}
    <fieldset>
      <legend>QSO</legend>
      <div class="grid-4">
        <div class="field">
          <label for={fid("call")}>Their callsign<span class="req">*</span></label>
          <input
            id={fid("call")}
            type="text"
            bind:value={form.ham.their_call}
            maxlength="20"
            list={fid("calls")}
            autocomplete="off"
            autocapitalize="characters"
            class="mono"
            aria-invalid={err("ham.their_call") ? "true" : undefined}
          />
          {#if err("ham.their_call")}<span class="field-error">{err("ham.their_call")}</span>{/if}
        </div>
        <DateTimeInput
          id={fid("qso-start")}
          label="QSO start (defaults to event time)"
          bind:value={form.ham.qso_start_utc}
          zone={app.zone}
          clearable
          error={err("ham.qso_start_utc")}
        />
        <DateTimeInput
          id={fid("qso-end")}
          label="QSO end"
          bind:value={form.ham.qso_end_utc}
          zone={app.zone}
          clearable
          error={err("ham.qso_end_utc")}
        />
        <div class="field">
          <label for={fid("freq")}>Frequency</label>
          <div class="row" style="gap:6px">
            <input
              id={fid("freq")}
              type="text"
              inputmode="decimal"
              bind:value={form.freqText}
              list={fid("freqs")}
              autocomplete="off"
              class="mono"
              aria-invalid={err("ham.frequency_hz") || freq.error ? "true" : undefined}
            />
            <select aria-label="Frequency unit" bind:value={form.freqUnit} style="width:84px">
              <option>kHz</option><option>MHz</option><option>GHz</option>
            </select>
          </div>
          {#if freq.error || err("ham.frequency_hz")}<span class="field-error"
              >{freq.error ?? err("ham.frequency_hz")}</span
            >{:else if suggestedBand && !form.ham.band}<button
              type="button"
              class="link small"
              onclick={() => (form.ham.band = suggestedBand!)}>Use band {suggestedBand}</button
            >{/if}
        </div>
        <div class="field">
          <label for={fid("band")}>Band</label>
          <select
            id={fid("band")}
            bind:value={form.ham.band}
            aria-invalid={err("ham.band") ? "true" : undefined}
          >
            <option value="">—</option>
            {#each BANDS as [b] (b)}<option value={b}>{b}</option>{/each}
          </select>
          {#if err("ham.band")}<span class="field-error">{err("ham.band")}</span>{/if}
        </div>
        <div class="field">
          <label for={fid("mode")}>Mode</label>
          <input
            id={fid("mode")}
            type="text"
            bind:value={form.ham.mode}
            list={fid("modes")}
            maxlength="20"
            class="mono"
            autocapitalize="characters"
          />
          <datalist id={fid("modes")}
            >{#each recents.mode ?? [] as m (m)}<option value={m}>recent</option
              >{/each}{#each MODES.filter((m) => !(recents.mode ?? []).includes(m)) as m (m)}<option value={m}
              ></option>{/each}</datalist
          >
        </div>
        <div class="field">
          <label for={fid("rsts")}>RST sent</label>
          <input id={fid("rsts")} type="text" bind:value={form.ham.rst_sent} maxlength="10" class="mono" />
        </div>
        <div class="field">
          <label for={fid("rstr")}>RST received</label>
          <input id={fid("rstr")} type="text" bind:value={form.ham.rst_rcvd} maxlength="10" class="mono" />
        </div>
        <div class="field">
          <label for={fid("tgrid")}>Their grid</label>
          <input
            id={fid("tgrid")}
            type="text"
            bind:value={form.ham.their_grid}
            maxlength="8"
            class="mono"
            aria-invalid={err("ham.their_grid") ? "true" : undefined}
          />
          {#if err("ham.their_grid")}<span class="field-error">{err("ham.their_grid")}</span>{/if}
        </div>
        <div class="field">
          <label for={fid("mgrid")}>My grid</label>
          <input id={fid("mgrid")} type="text" bind:value={form.ham.my_grid} maxlength="8" class="mono" />
        </div>
        <div class="field">
          <label for={fid("scall")}>Station callsign</label>
          <input
            id={fid("scall")}
            type="text"
            bind:value={form.ham.station_call}
            maxlength="20"
            class="mono"
          />
        </div>
        <div class="field">
          <label for={fid("ocall")}>Operator callsign</label>
          <input
            id={fid("ocall")}
            type="text"
            bind:value={form.ham.operator_call}
            maxlength="20"
            class="mono"
          />
        </div>
      </div>
      <div class="grid-4" style="margin-top:10px">
        <div class="field">
          <label for={fid("mypota")}>My POTA park</label>
          <input
            id={fid("mypota")}
            type="text"
            bind:value={form.ham.my_pota_ref}
            maxlength="17"
            list={fid("potas")}
            autocomplete="off"
            placeholder="US-0005"
            aria-invalid={err("ham.my_pota_ref") ? "true" : undefined}
          />
          {#if err("ham.my_pota_ref")}<span class="field-error">{err("ham.my_pota_ref")}</span>{/if}
        </div>
        <div class="field">
          <label for={fid("pota")}>Their POTA park (P2P)</label>
          <input
            id={fid("pota")}
            type="text"
            bind:value={form.ham.pota_ref}
            maxlength="17"
            list={fid("potas")}
            autocomplete="off"
            aria-invalid={err("ham.pota_ref") ? "true" : undefined}
          />
          {#if err("ham.pota_ref")}<span class="field-error">{err("ham.pota_ref")}</span>{/if}
        </div>
        <div class="field">
          <label for={fid("mysota")}>My SOTA summit</label>
          <input
            id={fid("mysota")}
            type="text"
            bind:value={form.ham.my_sota_ref}
            maxlength="20"
            list={fid("sotas")}
            autocomplete="off"
            placeholder="W4C/CM-001"
            aria-invalid={err("ham.my_sota_ref") ? "true" : undefined}
          />
          {#if err("ham.my_sota_ref")}<span class="field-error">{err("ham.my_sota_ref")}</span>{/if}
        </div>
        <div class="field">
          <label for={fid("sota")}>Their SOTA summit (S2S / chaser)</label>
          <input
            id={fid("sota")}
            type="text"
            bind:value={form.ham.sota_ref}
            maxlength="20"
            list={fid("sotas")}
            autocomplete="off"
            aria-invalid={err("ham.sota_ref") ? "true" : undefined}
          />
          {#if err("ham.sota_ref")}<span class="field-error">{err("ham.sota_ref")}</span>{/if}
        </div>
      </div>
      <div class="field" style="margin-top:10px">
        <label for={fid("qnotes")}>QSO notes</label>
        <input id={fid("qnotes")} type="text" bind:value={form.ham.qso_notes} maxlength="10000" />
      </div>
    </fieldset>
  {/if}

  {#if !compact}
    <details class="locs" open={!!(form.operator_location || form.reported_location)}>
      <summary
        >Location{#if form.operator_location || form.reported_location}<span class="muted">
            · {[
              form.operator_location ? "station position" : "",
              form.reported_location ? "reported place" : "",
            ]
              .filter(Boolean)
              .join(" + ")}</span
          >{/if}</summary
      >
      <div class="locs-body">
        <LocationEditor
          id={fid("oploc")}
          kind="station"
          bind:value={form.operator_location}
          defaultLocation={app.station?.default_location ?? null}
          error={err("operator_location.latitude_deg") ||
            err("operator_location.label") ||
            err("operator_location.maidenhead")}
        />
        <LocationEditor
          id={fid("reploc")}
          kind="reported"
          bind:value={form.reported_location}
          error={err("reported_location.latitude_deg") ||
            err("reported_location.label") ||
            err("reported_location.maidenhead")}
        />
        <span class="hint"
          >Optional. Never filled automatically; a missing or failed GPS fix never stops you saving.</span
        >
      </div>
    </details>
  {/if}

  {#if type === "emergency" && !compact}
    <fieldset>
      <legend>Incident</legend>
      <div class="grid-4">
        <div class="field">
          <label for={fid("inc")}>Incident</label><input
            id={fid("inc")}
            type="text"
            bind:value={form.em.incident_name}
            maxlength="200"
          />
        </div>
        <div class="field">
          <label for={fid("incn")}>Incident number</label><input
            id={fid("incn")}
            type="text"
            bind:value={form.em.incident_number}
            maxlength="200"
          />
        </div>
        <div class="field">
          <label for={fid("opid")}>Operational period</label>
          <select id={fid("opid")} bind:value={form.op_id}>
            <option value="">{app.periods.length ? "None — by event time" : "None defined"}</option>
            {#each periodChoices as p (p.id)}<option value={p.id}
                >{p.name}{p.status === "void" ? " (voided)" : ""}</option
              >{/each}
          </select>
        </div>
        <div class="field">
          <label for={fid("op")}>Period (text)</label><input
            id={fid("op")}
            type="text"
            bind:value={form.em.operational_period}
            maxlength="200"
          />
        </div>
      </div>
    </fieldset>
    {#if form.kind === "message"}
      <fieldset>
        <legend>Message details</legend>
        <p class="hint" style="margin:0 0 8px">
          These fields fill an ICS 213 when you export one from the Traffic Log or the entry's details.
        </p>
        <div class="grid-4">
          <div class="field">
            <label for={fid("msgno")}>Message number</label><input
              id={fid("msgno")}
              type="text"
              bind:value={form.em.message_number}
              maxlength="200"
            />
          </div>
          <div class="field">
            <label for={fid("prec")}>Precedence</label>
            <input
              id={fid("prec")}
              type="text"
              bind:value={form.em.precedence}
              list={fid("precs")}
              maxlength="200"
            />
            <datalist id={fid("precs")}
              ><option value="IMMEDIATE"></option><option value="PRIORITY"></option><option value="ROUTINE"
              ></option><option value="WELFARE"></option><option value="EMERGENCY"></option></datalist
            >
          </div>
          <DateTimeInput
            id={fid("sent")}
            label="Sent time"
            bind:value={form.em.sent_at_utc}
            zone={app.zone}
            clearable
          />
          <DateTimeInput
            id={fid("rcvd")}
            label="Received time"
            bind:value={form.em.received_at_utc}
            zone={app.zone}
            clearable
          />
          <div class="field">
            <label for={fid("orig")}>Originator (from)</label><input
              id={fid("orig")}
              type="text"
              bind:value={form.em.originator}
              maxlength="200"
            />
          </div>
          <div class="field">
            <label for={fid("origp")}>Originator position</label><input
              id={fid("origp")}
              type="text"
              bind:value={form.em.originator_position}
              maxlength="200"
            />
          </div>
          <div class="field">
            <label for={fid("recip")}>Recipient (to)</label><input
              id={fid("recip")}
              type="text"
              bind:value={form.em.recipient}
              maxlength="200"
            />
          </div>
          <div class="field">
            <label for={fid("recipp")}>Recipient position</label><input
              id={fid("recipp")}
              type="text"
              bind:value={form.em.recipient_position}
              maxlength="200"
            />
          </div>
          <div class="field">
            <label for={fid("sender")}>Sent by (operator)</label><input
              id={fid("sender")}
              type="text"
              bind:value={form.em.sender}
              maxlength="200"
            />
          </div>
          <div class="field">
            <label for={fid("receiver")}>Received by (operator)</label><input
              id={fid("receiver")}
              type="text"
              bind:value={form.em.receiver}
              maxlength="200"
            />
          </div>
          <div class="field">
            <label for={fid("disp")}>Disposition</label><input
              id={fid("disp")}
              type="text"
              bind:value={form.em.disposition}
              maxlength="200"
            />
          </div>
          <div class="field" style="justify-content:flex-end">
            <label class="check"
              ><input type="checkbox" bind:checked={form.em.reply_requested} /> Reply requested</label
            >
          </div>
        </div>
        <div class="grid-2" style="margin-top:10px">
          <div class="field">
            <label for={fid("act")}>Requested action / reply</label><textarea
              id={fid("act")}
              bind:value={form.em.action_requested}
              rows="2"></textarea>
          </div>
          <div class="field">
            <label for={fid("resp")}>Response (ICS 213 reply)</label><textarea
              id={fid("resp")}
              bind:value={form.em.response}
              rows="2"></textarea>
          </div>
        </div>
        <div class="grid-4" style="margin-top:10px">
          <div class="field">
            <label for={fid("apn")}>Approved by — name</label><input
              id={fid("apn")}
              type="text"
              bind:value={form.em.approved_by_name}
              maxlength="200"
            />
          </div>
          <div class="field">
            <label for={fid("app")}>Approved by — position/title</label><input
              id={fid("app")}
              type="text"
              bind:value={form.em.approved_by_position}
              maxlength="200"
            />
          </div>
          <div class="field">
            <label for={fid("rbn")}>Replied by — name</label><input
              id={fid("rbn")}
              type="text"
              bind:value={form.em.replied_by_name}
              maxlength="200"
            />
          </div>
          <div class="field">
            <label for={fid("rbp")}>Replied by — position/title</label><input
              id={fid("rbp")}
              type="text"
              bind:value={form.em.replied_by_position}
              maxlength="200"
            />
          </div>
          <DateTimeInput
            id={fid("rat")}
            label="Replied at"
            bind:value={form.em.replied_at_utc}
            zone={app.zone}
            clearable
          />
        </div>
      </fieldset>
    {:else if form.kind === "activity"}
      <fieldset>
        <legend>Activity details</legend>
        <p class="hint" style="margin:0 0 8px">
          Activity entries make up the ICS 214 activity log for their operational period.
        </p>
        <div class="grid-4">
          <div class="field">
            <label for={fid("unit")}>Resource / unit</label><input
              id={fid("unit")}
              type="text"
              bind:value={form.em.resource_unit}
              maxlength="200"
            />
          </div>
          <div class="field">
            <label for={fid("assign")}>Assignment</label><input
              id={fid("assign")}
              type="text"
              bind:value={form.em.assignment}
              maxlength="200"
            />
          </div>
          <DateTimeInput
            id={fid("astart")}
            label="Activity start (defaults to event time)"
            bind:value={form.em.activity_start_utc}
            zone={app.zone}
            clearable
            error={err("emergency.activity_start_utc")}
          />
          <DateTimeInput
            id={fid("aend")}
            label="Activity end"
            bind:value={form.em.activity_end_utc}
            zone={app.zone}
            clearable
            error={err("emergency.activity_end_utc")}
          />
          <div class="field">
            <label for={fid("aloc")}>Location</label><input
              id={fid("aloc")}
              type="text"
              bind:value={form.em.location}
              maxlength="200"
            />
          </div>
          <div class="field">
            <label for={fid("adisp")}>Disposition</label><input
              id={fid("adisp")}
              type="text"
              bind:value={form.em.disposition}
              maxlength="200"
            />
          </div>
          <div class="field">
            <label for={fid("prep")}>Prepared by</label><input
              id={fid("prep")}
              type="text"
              bind:value={form.em.preparer}
              maxlength="200"
            />
          </div>
        </div>
      </fieldset>
    {/if}
  {/if}

  {#if templates.length && mode === "new" && !compact}
    <div class="row wrap templates" role="group" aria-label="Quick templates">
      <span class="hint">Quick templates:</span>
      {#each templates.slice(0, 8) as t (t.id)}
        <button type="button" class="small" onclick={() => useTemplate(t)}>{t.name}</button>
      {/each}
    </div>
  {/if}

  <div class="grid-3" hidden={compact}>
    {#if !hidden.has("tags")}
      <TagInput
        id={fid("tags")}
        label={lbl("tags", "Tags")}
        bind:tags={form.tags}
        quick={preset.quick_tags}
      />
    {/if}
    {#if !hidden.has("location")}
      <div class="field">
        <label for={fid("loc")}>{lbl("location", "Location")}</label>
        <input id={fid("loc")} type="text" bind:value={form.location} maxlength="200" />
      </div>
    {/if}
    <div class="field">
      <span class="label">Options</span>
      {#if !hidden.has("flagged")}<label class="check"
          ><input type="checkbox" bind:checked={form.flagged} /> Flag for report</label
        >{/if}
      {#if !hidden.has("attachments")}
        <button type="button" class="small" onclick={attach} disabled={blocked}
          ><Paperclip size={14} />Attach file…</button
        >
      {/if}
    </div>
  </div>
  {#if mode === "edit" && entry?.attachments.length}
    <p class="hint" style="margin:0">
      Existing attachments: {entry.attachments.map((a) => a.display_name).join(", ")} (kept)
    </p>
  {/if}
  {#if form.attachments.length}
    <ul class="atts" aria-label="New attachments">
      {#each form.attachments as a (a.staging_id)}
        <li class="badge neutral">
          <Paperclip size={12} />{a.display_name}
          <span class="muted">({Math.ceil(a.size_bytes / 1024)} KB)</span>
          <button
            type="button"
            class="ghost icon-only small"
            aria-label="Remove attachment {a.display_name}"
            onclick={() => removeAttachment(a)}><X size={12} /></button
          >
        </li>
      {/each}
    </ul>
  {/if}

  {#if mode === "edit"}
    <div class="field">
      <label for={fid("reason")}>Reason for change (optional, kept in revision history)</label>
      <input id={fid("reason")} type="text" bind:value={reason} maxlength="500" />
    </div>
  {/if}

  {#if saveError}
    <div class="banner error" role="alert">
      <CircleAlert size={18} class="icon" />
      <div class="grow">
        <strong>{saveError.code === "VALIDATION" ? "Not saved:" : "Save Failed:"}</strong>
        {saveError.message}
        <div class="hint">
          {saveError.code === "VALIDATION" ? "Correct the highlighted field and save again. " : ""}Nothing was
          added to the logbook. Your entry is still here{app.draftStatus.kind === "saved"
            ? " and stored as a recovery draft"
            : ""}.
        </div>
      </div>
      {#if !app.storageBlocked}<button class="small" onclick={save} disabled={saving}
          ><RefreshCw size={14} />Retry</button
        >{/if}
    </div>
  {/if}

  <div class="bar">
    <div class="status" aria-hidden="true">
      {#if saving}<LoaderCircle size={15} class="spin" /> Saving…
      {:else if dirty && app.draftStatus.kind === "failed"}<CircleAlert size={15} class="err" /> Draft Save Failed
        — this entry is not saved
      {:else if dirty && app.draftStatus.kind === "saved"}<FileClock size={15} class="warn" /> Not saved yet · draft
        kept
        {formatInstant(app.draftStatus.at, app.zone, app.clock, { date: false, seconds: true })}
      {:else if dirty}<FileClock size={15} class="warn" /> Not saved yet
      {:else if app.saveStatus.kind === "saved"}<CircleCheck size={15} class="ok" />
        {app.saveStatus.edit ? "Changes Saved" : "Entry Saved"} #{app.saveStatus.seq} at {formatInstant(
          app.saveStatus.at,
          app.zone,
          app.clock,
          { date: false, seconds: true },
        )}{#if app.saveStatus.duplicate}
          (already saved){/if}{/if}
    </div>
    <span class="spacer"></span>
    {#if mode === "new"}
      <button type="button" onclick={clearForm} disabled={saving}><Eraser size={16} />Clear</button>
      {#if !compact}
        <button type="button" onclick={saveAsTemplate} disabled={saving || app.readOnly}
          ><BookmarkPlus size={16} />Save as Template</button
        >
        <label class="check"><input type="checkbox" bind:checked={addAnother} /> Add another after save</label
        >
      {/if}
      <button
        type="button"
        class="success"
        onclick={async () => {
          await save();
          if (!addAnother && !saveError) app.navigate("log");
        }}
        disabled={saving || blocked}
        aria-keyshortcuts={modKey() === "⌘" ? "Meta+Enter" : "Control+Enter"}
      >
        <Save size={16} />Save Entry ({modKey()}{modKey() === "⌘" ? "↩" : "Enter"})
      </button>
    {:else}
      <button type="button" onclick={cancelEdit} disabled={saving}>Cancel</button>
      <button
        type="button"
        class="success"
        onclick={save}
        disabled={saving || blocked}
        aria-keyshortcuts={modKey() === "⌘" ? "Meta+Enter" : "Control+Enter"}
      >
        <Save size={16} />Save Changes ({modKey()}{modKey() === "⌘" ? "↩" : "Enter"})
      </button>
    {/if}
  </div>
</div>

<!-- Local recent values (this logbook only); applied only when picked. -->
<datalist id={fid("stations")}
  >{#each recents.station ?? [] as v (v)}<option value={v}></option>{/each}</datalist
>
<datalist id={fid("calls")}
  >{#each recents.their_call ?? [] as v (v)}<option value={v}></option>{/each}</datalist
>
<datalist id={fid("freqs")}
  >{#each recentFreqs as v (v)}<option value={v}></option>{/each}</datalist
>
<datalist id={fid("potas")}
  >{#each recents.pota_ref ?? [] as v (v)}<option value={v}></option>{/each}</datalist
>
<datalist id={fid("sotas")}
  >{#each recents.sota_ref ?? [] as v (v)}<option value={v}></option>{/each}</datalist
>

<style>
  .locs {
    border: 1px solid var(--border);
    border-radius: 9px;
    padding: 8px 12px;
  }
  .locs summary {
    cursor: pointer;
    font-weight: 600;
    font-size: 13px;
  }
  .locs-body {
    display: flex;
    flex-direction: column;
    gap: 12px;
    margin-top: 10px;
  }
  .editor {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .editor.compact :global(.grid-4) {
    grid-template-columns: 1fr 1fr;
  }
  .label {
    color: var(--text-2);
    font-size: 13px;
  }
  .bar {
    display: flex;
    gap: 10px;
    align-items: center;
    flex-wrap: wrap;
    padding-top: 10px;
    border-top: 1px solid var(--border);
  }
  .status {
    display: flex;
    gap: 6px;
    align-items: center;
    color: var(--text-2);
    font-size: 13px;
  }
  .status :global(.ok) {
    color: var(--ok);
  }
  .status :global(.warn) {
    color: var(--warn);
  }
  .status :global(.err) {
    color: var(--danger);
  }
  .atts {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .templates {
    gap: 6px;
  }
</style>
