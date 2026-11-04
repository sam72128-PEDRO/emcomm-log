<script lang="ts">
  import { CircleAlert } from "@lucide/svelte";
  import Modal from "./Modal.svelte";
  import LocationEditor from "./LocationEditor.svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import type * as T from "../lib/types";

  let { onclose }: { onclose: () => void } = $props();

  const isHam = app.bookType === "ham";
  const s0 = app.station;
  let st = $state<T.Station>({
    label: s0?.label ?? "",
    tactical_id: s0?.tactical_id ?? "",
    station_callsign: s0?.station_callsign ?? "",
    agency: s0?.agency ?? "",
    default_location: s0?.default_location ?? null,
    pota_ref: s0?.pota_ref ?? "",
    sota_ref: s0?.sota_ref ?? "",
    updated_at_utc: s0?.updated_at_utc ?? "",
  });
  let busy = $state(false);
  let error = $state<T.AppError | null>(null);
  const err = (f: string) => (error?.field === `station.${f}` ? error.message : "");

  async function save() {
    busy = true;
    error = null;
    try {
      app.station = await api.setStation($state.snapshot(st) as T.Station);
      app.toast("ok", "Station saved. New entries use it; entries already saved keep theirs.");
      onclose();
    } catch (e) {
      error = toAppError(e);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title="Station / assignment" {onclose} {busy} size="lg">
  <p class="muted" style="margin:0">
    The station is the desk, position or portable setup — not the person. Several operators can work the same
    station in turn (Switch Operator). A tactical identifier is not a licensed callsign; enter the licensed
    station callsign separately if one is used on the air.
  </p>
  {#if error && !error.field?.startsWith("station.")}<div class="banner error" role="alert">
      <CircleAlert size={18} class="icon" />
      <div>{error.message}</div>
    </div>{/if}
  <div class="grid-2">
    <div class="field">
      <label for="st-tac">Tactical identifier</label>
      <input
        id="st-tac"
        type="text"
        bind:value={st.tactical_id}
        maxlength="120"
        placeholder="e.g. MARC-5 COMMS"
      />
      {#if err("tactical_id")}<span class="field-error">{err("tactical_id")}</span>{/if}
    </div>
    <div class="field">
      <label for="st-label">Station / position label</label>
      <input
        id="st-label"
        type="text"
        bind:value={st.label}
        maxlength="120"
        placeholder="e.g. EOC radio room desk 2"
      />
    </div>
    <div class="field">
      <label for="st-call">Licensed station callsign (optional)</label>
      <input
        id="st-call"
        type="text"
        bind:value={st.station_callsign}
        maxlength="20"
        placeholder={isHam ? "e.g. W4CLB (club) or your own" : "Only if used on the air"}
        aria-invalid={err("station_callsign") ? "true" : undefined}
      />
      {#if err("station_callsign")}<span class="field-error">{err("station_callsign")}</span>{/if}
    </div>
    <div class="field">
      <label for="st-ag">Agency / jurisdiction</label>
      <input id="st-ag" type="text" bind:value={st.agency} maxlength="200" />
    </div>
    {#if isHam}
      <div class="field">
        <label for="st-pota">Activating POTA park (default for new QSOs)</label>
        <input
          id="st-pota"
          type="text"
          bind:value={st.pota_ref}
          maxlength="17"
          placeholder="e.g. US-0005"
          aria-invalid={err("pota_ref") ? "true" : undefined}
        />
        {#if err("pota_ref")}<span class="field-error">{err("pota_ref")}</span>{/if}
      </div>
      <div class="field">
        <label for="st-sota">Activating SOTA summit (default for new QSOs)</label>
        <input
          id="st-sota"
          type="text"
          bind:value={st.sota_ref}
          maxlength="20"
          placeholder="e.g. W4C/CM-001"
          aria-invalid={err("sota_ref") ? "true" : undefined}
        />
        {#if err("sota_ref")}<span class="field-error">{err("sota_ref")}</span>{/if}
      </div>
    {/if}
  </div>
  <fieldset>
    <legend>Default station position</legend>
    <LocationEditor
      id="st-loc"
      kind="station"
      bind:value={st.default_location}
      error={error?.field?.startsWith("station.default_location") ? error.message : ""}
    />
    <span class="hint"
      >Offered as "Use default" on each entry. Changing it later never changes saved entries.</span
    >
  </fieldset>
  {#snippet footer()}
    <button onclick={onclose}>Cancel</button>
    <button class="primary" onclick={save} disabled={busy}>Save station</button>
  {/snippet}
</Modal>
