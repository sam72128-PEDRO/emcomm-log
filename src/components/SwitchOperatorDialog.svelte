<script lang="ts">
  import { CircleAlert } from "@lucide/svelte";
  import Modal from "./Modal.svelte";
  import DateTimeInput from "./DateTimeInput.svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import type * as T from "../lib/types";

  let { onclose }: { onclose: () => void } = $props();

  const outgoing = app.shift?.operator ?? null;
  const choices = $derived(app.profiles.filter((p) => p.active && p.id !== app.shift?.profile_id));
  let profileId = $state("");
  let at = $state<string | null>(null);
  let note = $state("");
  // An unsaved entry at handover is never silently re-attributed (2.1 spec §2.3).
  let draftChoice = $state<"save" | "reassign">("save");
  let busy = $state(false);
  let error = $state<T.AppError | null>(null);
  const dirty = app.editorDirty;
  const incoming = $derived(app.profiles.find((p) => p.id === profileId) ?? null);

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

<Modal title="Switch operator" {onclose} {busy}>
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
          : "No shift yet"}</strong
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
  {#snippet footer()}
    <button onclick={onclose}>Cancel</button>
    <button class="primary" onclick={confirmSwitch} disabled={busy}>Switch operator</button>
  {/snippet}
</Modal>
