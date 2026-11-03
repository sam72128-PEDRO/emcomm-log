<script lang="ts">
  import { fromInputValue, toInputValue, zoneLabel, nowIso } from "../lib/time";
  import type { Zone } from "../lib/time";

  let {
    value = $bindable(),
    zone,
    id,
    label,
    required = false,
    error = "",
    nowButton = true,
    clearable = false,
    disabled = false,
    onchange,
  }: {
    value: string | null;
    zone: Zone;
    id: string;
    label: string;
    required?: boolean;
    error?: string;
    nowButton?: boolean;
    clearable?: boolean;
    disabled?: boolean;
    onchange?: () => void;
  } = $props();

  let text = $state("");
  let lastValue: string | null = null;
  $effect(() => {
    if (value !== lastValue) {
      text = toInputValue(value, zone);
      lastValue = value;
    }
  });

  function commit() {
    const iso = fromInputValue(text, zone);
    if (!text) {
      value = null;
    } else if (iso) {
      value = iso;
    }
    lastValue = value;
    onchange?.();
  }

  function setNow() {
    value = nowIso();
    lastValue = null;
    onchange?.();
  }
</script>

<div class="field">
  <label for={id}
    >{label}{#if required}<span class="req" aria-hidden="true">*</span>{/if}</label
  >
  <div class="row" style="gap:6px">
    <input
      {id}
      type="datetime-local"
      step="1"
      bind:value={text}
      onchange={commit}
      onblur={commit}
      {required}
      {disabled}
      aria-invalid={error ? "true" : undefined}
      aria-describedby="{id}-zone{error ? ` ${id}-err` : ''}"
    />
    {#if nowButton}<button type="button" class="small" onclick={setNow} {disabled}>Now</button>{/if}
    {#if clearable && value}<button
        type="button"
        class="small ghost"
        onclick={() => {
          value = null;
          lastValue = null;
          onchange?.();
        }}
        {disabled}>Clear</button
      >{/if}
  </div>
  <span class="hint" id="{id}-zone"
    >{value ? "" : "Not set · "}{zoneLabel(zone, value ? new Date(value) : new Date())}</span
  >
  {#if error}<span class="field-error" id="{id}-err">{error}</span>{/if}
</div>
