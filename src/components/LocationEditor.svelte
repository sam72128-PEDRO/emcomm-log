<script lang="ts">
  import { Crosshair, Eraser, House } from "@lucide/svelte";
  import { app } from "../lib/state.svelte";
  import { describeSource, formatCoord, parseCoord } from "../lib/location";
  import { formatInstant } from "../lib/time";
  import type * as T from "../lib/types";
  import GpsCaptureDialog from "./GpsCaptureDialog.svelte";

  let {
    value = $bindable(),
    id,
    kind,
    defaultLocation = null,
    error = "",
  }: {
    value: T.Location | null;
    id: string;
    /** "station": where the logging station is (may come from a receiver); "reported": a place in the traffic. */
    kind: "station" | "reported";
    defaultLocation?: T.Location | null;
    error?: string;
  } = $props();

  let capturing = $state(false);
  let latText = $state("");
  let lonText = $state("");
  let coordError = $state("");
  let last: T.Location | null | undefined = undefined;
  $effect(() => {
    if (value !== last) {
      latText = value?.latitude_deg != null ? String(value.latitude_deg) : "";
      lonText = value?.longitude_deg != null ? String(value.longitude_deg) : "";
      last = value;
    }
  });

  function blank(): T.Location {
    return {
      latitude_deg: null,
      longitude_deg: null,
      label: "",
      maidenhead: "",
      source: kind === "reported" ? "reported" : "manual",
      provider_id: "",
      captured_at_utc: null,
      accuracy_m: null,
      altitude_m: null,
      satellites: null,
      notes: "",
    };
  }

  /** Typed edits make the value a manual one: a receiver's fix is only kept as captured. */
  function edit(patch: Partial<T.Location>) {
    const base = value ?? blank();
    const manual = kind === "reported" ? "reported" : "manual";
    const next: T.Location = { ...base, ...patch };
    if ("latitude_deg" in patch || "longitude_deg" in patch) {
      Object.assign(next, {
        source: manual,
        provider_id: "",
        captured_at_utc: null,
        accuracy_m: null,
        satellites: null,
        altitude_m: null,
      });
    }
    const empty =
      next.latitude_deg == null && !next.label.trim() && !next.maidenhead.trim() && !next.notes.trim();
    value = empty ? null : next;
    last = value;
  }

  function commitCoords() {
    coordError = "";
    if (!latText.trim() && !lonText.trim()) {
      edit({ latitude_deg: null, longitude_deg: null });
      return;
    }
    const lat = parseCoord(latText, "lat");
    const lon = parseCoord(lonText, "lon");
    if (lat === null || lon === null) {
      coordError = "Enter both as decimal degrees, e.g. 33.812100 and -84.401100 (or 33.8121 N, 84.4011 W)";
      return;
    }
    edit({ latitude_deg: lat, longitude_deg: lon });
  }
</script>

<div class="loc" role="group" aria-labelledby="{id}-legend">
  <div class="row wrap head">
    <span id="{id}-legend" class="legend"
      >{kind === "station" ? "Station position" : "Reported location"}</span
    >
    {#if value}<span class="badge neutral" title={value.provider_id || undefined}
        >{describeSource(value.source)}</span
      >{/if}
    <span class="spacer"></span>
    {#if kind === "station"}
      <button
        class="small ghost"
        onclick={() => (capturing = true)}
        title="Get one fix from a GPS receiver or the computer's location"
        ><Crosshair size={14} />Capture GPS…</button
      >
      {#if defaultLocation}<button
          class="small ghost"
          onclick={() => {
            value = {
              ...defaultLocation!,
              source: "logbook_default",
              provider_id: `${defaultLocation!.source} ${defaultLocation!.provider_id}`.trim(),
            };
            last = value;
          }}
          title="Use this logbook's default station position"><House size={14} />Use default</button
        >{/if}
    {/if}
    {#if value}<button class="small ghost" onclick={() => ((value = null), (last = null))}
        ><Eraser size={14} />Clear</button
      >{/if}
  </div>
  <div class="grid-4">
    <div class="field">
      <label for="{id}-label">Place name</label>
      <input
        id="{id}-label"
        type="text"
        value={value?.label ?? ""}
        oninput={(e) => edit({ label: e.currentTarget.value })}
        maxlength="200"
        placeholder={kind === "reported" ? "e.g. CR-12 bridge at Pine Rd" : "e.g. EOC parking lot"}
      />
    </div>
    <div class="field">
      <label for="{id}-lat">Latitude</label>
      <input
        id="{id}-lat"
        type="text"
        inputmode="decimal"
        bind:value={latText}
        onchange={commitCoords}
        placeholder="33.812100"
        aria-invalid={coordError ? "true" : undefined}
      />
    </div>
    <div class="field">
      <label for="{id}-lon">Longitude</label>
      <input
        id="{id}-lon"
        type="text"
        inputmode="decimal"
        bind:value={lonText}
        onchange={commitCoords}
        placeholder="-84.401100"
        aria-invalid={coordError ? "true" : undefined}
      />
    </div>
    <div class="field">
      <label for="{id}-grid">Grid</label>
      <input
        id="{id}-grid"
        type="text"
        value={value?.maidenhead ?? ""}
        oninput={(e) => edit({ maidenhead: e.currentTarget.value })}
        maxlength="8"
        placeholder="EM73wv"
      />
    </div>
  </div>
  {#if coordError || error}<span class="field-error">{coordError || error}</span>{/if}
  {#if value && value.latitude_deg != null}
    <span class="hint"
      >{formatCoord(value.latitude_deg, value.longitude_deg)}{#if value.captured_at_utc}
        · fix {formatInstant(value.captured_at_utc, app.zone, app.clock)}{/if}{#if value.satellites != null}
        · {value.satellites} satellites{/if}{value.accuracy_m != null
        ? ` · ±${Math.round(value.accuracy_m)} m`
        : value.source === "manual" || value.source === "reported"
          ? ""
          : " · accuracy unknown"}</span
    >
  {/if}
</div>

{#if capturing}<GpsCaptureDialog
    onaccept={(l) => {
      value = l;
      last = l;
      capturing = false;
    }}
    onclose={() => (capturing = false)}
  />{/if}

<style>
  .loc {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .legend {
    font-weight: 600;
    font-size: 13px;
  }
  .head {
    gap: 6px;
  }
</style>
