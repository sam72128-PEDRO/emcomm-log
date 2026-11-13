<script lang="ts">
  import { CircleAlert, ClipboardList, Copy } from "@lucide/svelte";
  import Modal from "./Modal.svelte";
  import DateTimeInput from "./DateTimeInput.svelte";
  import HandoverSummaryView from "./HandoverSummaryView.svelte";
  import { copyText } from "../lib/clipboard";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import type * as T from "../lib/types";

  let { onclose }: { onclose: () => void } = $props();

  // Before the first entry no shift is recorded yet; the operator shown in the header is on duty.
  const outgoing: { name: string; callsign: string } | null =
    app.shift?.operator ?? (app.profile ? { name: app.profile.name, callsign: app.profile.callsign } : null);
  const onDutyId = app.shift?.profile_id ?? app.profile?.id;
  const choices = $derived(app.profiles.filter((p) => p.active && p.id !== onDutyId));
  let profileId = $state("");
  let at = $state<string | null>(null);
  let note = $state("");
  // An unsaved entry at handover is never silently re-attributed (2.1 spec §2.3).
  let draftChoice = $state<"save" | "reassign">("save");
  let busy = $state(false);
  let error = $state<T.AppError | null>(null);
  const dirty = app.editorDirty;
  const incoming = $derived(app.profiles.find((p) => p.id === profileId) ?? null);

  // 2.3: optional, read-only handover summary. Switching never depends on it.
  let view = $state<"form" | "summary">("form");
  let summary = $state<T.HandoverSummary | null>(null);
  let summaryError = $state<string | null>(null);
  let summaryBusy = $state(false);

  async function previewSummary() {
    summaryBusy = true;
    summaryError = null;
    summary = null;
    view = "summary";
    try {
      summary = await api.handoverPreview(profileId || null, at, note);
    } catch (e) {
      summaryError = toAppError(e).message;
    } finally {
      summaryBusy = false;
    }
  }

  async function copySummary() {
    if (!summary) return;
    if (await copyText(summary.text)) app.toast("ok", "Handover summary copied to the clipboard");
    else app.toast("error", "Could not copy to the clipboard");
  }

  /** Go to a referenced entry: the switch is not made (Cancel / Return to Log). */
  function openEntry(id: string) {
    onclose();
    void app.navigate("log");
    app.detailEntryId = id;
  }

  async function saveOutgoingDraft(): Promise<boolean> {
    const ed = app.topEditor;
    if (!ed?.dirty()) return true;
    ed.save();
    for (let i = 0; i < 40; i++) {
      await new Promise((r) => setTimeout(r, 100));
      if (!ed.dirty()) return true;
    }
    return false;
  }

  async function confirmSwitch() {
    if (!profileId) {
      error = { code: "VALIDATION", message: "Choose the incoming operator", field: "operator" };
      return;
    }
    busy = true;
    error = null;
    try {
      if (dirty && draftChoice === "save" && !(await saveOutgoingDraft())) {
        error = {
          code: "VALIDATION",
          message: "The unsaved entry could not be saved. Finish or correct it, then switch operators.",
        };
        return;
      }
      const s = await api.switchOperator(profileId, at, note);
      await app.refreshIdentity();
      app.toast("ok", `${s.operator.name} is now on duty${outgoing ? `, relieving ${outgoing.name}` : ""}`);
      app.announce(`Operator changed to ${s.operator.name}`);
      onclose();
    } catch (e) {
      error = toAppError(e);
    } finally {
      busy = false;
    }
  }
</script>

<Modal
  title={view === "summary" ? "Handover summary" : "Switch operator"}
  {onclose}
  {busy}
  size={view === "summary" ? "lg" : "md"}
  dirty={() => !!profileId || !!note.trim() || at !== null}
>
  {#if view === "summary"}
    {#if summaryError}<div class="banner error" role="alert">
        <CircleAlert size={18} class="icon" />
        <div>
          The summary could not be generated: {summaryError}<br />You can still switch operators.
        </div>
      </div>{/if}
    {#if summaryBusy}<p class="muted">Preparing summary…</p>{/if}
    {#if summary}<HandoverSummaryView {summary} onopen={openEntry} />{/if}
    {#if error}<div class="banner error" role="alert">
        <CircleAlert size={18} class="icon" />
        <div>{error.message}</div>
      </div>{/if}
  {:else}
    <p class="muted" style="margin:0">
      Same logbook, same station{app.station?.tactical_id ? ` (${app.station.tactical_id})` : ""}. Entries
      already saved keep the operator who logged them. The operational period and ICS forms are not changed.
    </p>
    {#if error}<div class="banner error" role="alert">
        <CircleAlert size={18} class="icon" />
        <div>{error.message}</div>
      </div>{/if}
    <div class="grid-2">
      <div class="field">
        <span class="label">On duty now</span>
        <strong
          >{outgoing
            ? `${outgoing.name}${outgoing.callsign ? ` · ${outgoing.callsign}` : ""}`
            : "No operator"}</strong
        >
      </div>
      <div class="field">
        <label for="sw-op">Incoming operator<span class="req">*</span></label>
        <select
          id="sw-op"
          bind:value={profileId}
          aria-invalid={error?.field === "operator" ? "true" : undefined}
        >
          <option value="">Choose…</option>
          {#each choices as p (p.id)}<option value={p.id}
              >{p.name}{p.callsign ? ` · ${p.callsign}` : ""}{p.license_class
                ? ` (${p.license_class})`
                : ""}</option
            >{/each}
        </select>
        <button
          class="small link"
          onclick={() => {
            onclose();
            app.navigate("settings");
          }}>Add or edit operator profiles…</button
        >
      </div>
    </div>
    <div class="grid-2">
      <DateTimeInput
        id="sw-at"
        label="Handover time (leave empty for now)"
        bind:value={at}
        zone={app.zone}
        clearable
      />
      <div class="field">
        <label for="sw-note">Note</label>
        <input
          id="sw-note"
          type="text"
          bind:value={note}
          maxlength="200"
          placeholder="e.g. Shift change 1900"
        />
      </div>
    </div>
    {#if dirty}
      <fieldset>
        <legend>Unsaved entry in progress</legend>
        <label class="check"
          ><input type="radio" name="sw-draft" value="save" bind:group={draftChoice} /> Save it now as {outgoing?.name ??
            "the current operator"}</label
        >
        <label class="check"
          ><input type="radio" name="sw-draft" value="reassign" bind:group={draftChoice} /> Leave it open for {incoming?.name ??
            "the incoming operator"} to finish (it will be recorded under them)</label
        >
      </fieldset>
    {/if}
  {/if}
  {#snippet footer()}
    {#if view === "summary"}
      <button onclick={() => (view = "form")} disabled={busy}>Back</button>
      <button onclick={copySummary} disabled={!summary}><Copy size={14} />Copy Summary</button>
      <button onclick={onclose} disabled={busy}>Cancel / Return to Log</button>
      <button class="primary" onclick={confirmSwitch} disabled={busy || !profileId}
        >{profileId ? "Continue Operator Switch" : "Choose the incoming operator first"}</button
      >
    {:else}
      <button onclick={previewSummary} disabled={busy}><ClipboardList size={14} />Handover Summary…</button>
      <span class="spacer"></span>
      <button onclick={onclose}>Cancel</button>
      <button class="primary" onclick={confirmSwitch} disabled={busy}>Switch operator</button>
    {/if}
  {/snippet}
</Modal>
