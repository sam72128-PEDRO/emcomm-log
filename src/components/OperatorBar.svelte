<script lang="ts">
  import { UserRound, RadioTower, ArrowRightLeft } from "@lucide/svelte";
  import { app } from "../lib/state.svelte";
  import SwitchOperatorDialog from "./SwitchOperatorDialog.svelte";
  import StationDialog from "./StationDialog.svelte";

  let { compact = false }: { compact?: boolean } = $props();
  let switching = $state(false);
  let editingStation = $state(false);

  const st = $derived(app.station);
  const stationText = $derived(
    st ? st.tactical_id || st.label || st.station_callsign || "Not set" : "Not set",
  );
  const who = $derived(app.shift?.operator ?? app.profile ?? null);
</script>

<div class="opbar" class:compact role="group" aria-label="Station and operator">
  <button
    class="small ghost chip"
    onclick={() => (editingStation = true)}
    disabled={app.readOnly}
    title="Station / assignment for this logbook (separate from the person operating)"
    ><RadioTower size={14} />Station: <strong>{stationText}</strong
    >{#if st?.station_callsign && st.station_callsign !== stationText}<span class="muted">
        · {st.station_callsign}</span
      >{/if}</button
  >
  <span class="chip plain"
    ><UserRound size={14} />Operator: <strong>{who?.name ?? "—"}</strong>{#if who?.callsign}<span class="muted">
        · {who.callsign}</span
      >{/if}</span
  >
  <button class="small" onclick={() => (switching = true)} disabled={app.readOnly}
    ><ArrowRightLeft size={14} />Switch Operator</button
  >
</div>

{#if switching}<SwitchOperatorDialog onclose={() => (switching = false)} />{/if}
{#if editingStation}<StationDialog onclose={() => (editingStation = false)} />{/if}

<style>
  .opbar {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 10px;
    align-items: center;
    font-size: 13px;
  }
  .chip {
    display: inline-flex;
    gap: 5px;
    align-items: center;
  }
  .plain {
    padding: 4px 6px;
  }
  .compact {
    font-size: 12.5px;
  }
</style>
