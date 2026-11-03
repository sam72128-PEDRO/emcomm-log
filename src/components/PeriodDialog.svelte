<script lang="ts">
  import { untrack } from "svelte";
  import Modal from "./Modal.svelte";
  import DateTimeInput from "./DateTimeInput.svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import type * as T from "../lib/types";

  let {
    period = null,
    suggestedStart = null,
    onclose,
  }: { period?: T.OperationalPeriod | null; suggestedStart?: string | null; onclose: () => void } = $props();

  const hour = 3_600_000;
  // The dialog edits a copy taken when it opens.
  const initial = untrack(() => period);
  const startDefault =
    untrack(() => suggestedStart) ?? new Date(Math.floor(Date.now() / hour) * hour).toISOString();
  let name = $state(initial?.name ?? `OP ${app.periods.filter((p) => p.status === "active").length + 1}`);
  let start = $state<string | null>(initial?.start_utc ?? startDefault);
  let end = $state<string | null>(
    initial?.end_utc ?? new Date(Date.parse(startDefault) + 12 * hour).toISOString(),
  );
  let notes = $state(initial?.notes ?? "");
  let busy = $state(false);
  let errors = $state<Record<string, string>>({});

  async function save() {
    errors = {};
    if (!name.trim()) errors.name = "Name is required";
    if (!start) errors.start_utc = "Start is required";
    if (!end) errors.end_utc = "End is required";
    if (start && end && Date.parse(end) <= Date.parse(start))
      errors.end_utc = "The period must end after it starts";
    if (Object.keys(errors).length) return;
    busy = true;
    try {
      const r = await api.savePeriod(period?.id ?? null, { name, start_utc: start!, end_utc: end!, notes });
      await app.refreshPeriods();
      app.toast("ok", `Operational period “${r.period.name}” saved`);
      if (r.overlaps.length) {
        app.toast(
          "warn",
          `“${r.period.name}” overlaps ${r.overlaps.map((n) => `“${n}”`).join(", ")}. That is allowed (handover), but check the times.`,
          10000,
        );
      }
      onclose();
    } catch (e) {
      const err = toAppError(e);
      if (err.field) errors[err.field] = err.message;
      else app.toast("error", err.message);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title={period ? "Edit operational period" : "New operational period"} {onclose} {busy}>
  <div class="field">
    <label for="op-name">Name<span class="req">*</span></label>
    <input
      id="op-name"
      type="text"
      bind:value={name}
      maxlength="80"
      aria-invalid={errors.name ? "true" : undefined}
      aria-describedby={errors.name ? "op-name-err" : undefined}
    />
    {#if errors.name}<span class="field-error" id="op-name-err">{errors.name}</span>{/if}
  </div>
  <div class="grid-2">
    <DateTimeInput
      id="op-start"
      label="Starts"
      bind:value={start}
      zone={app.zone}
      required
      error={errors.start_utc}
    />
    <DateTimeInput
      id="op-end"
      label="Ends"
      bind:value={end}
      zone={app.zone}
      required
      error={errors.end_utc}
      nowButton={false}
    />
  </div>
  <div class="field">
    <label for="op-notes">Notes</label>
    <textarea id="op-notes" rows="3" bind:value={notes} maxlength="10000"></textarea>
  </div>
  <p class="hint" style="margin:0">
    New entries are tagged with the period you are in. Periods may overlap for a handover.
  </p>
  {#snippet footer()}
    <button onclick={onclose}>Cancel</button>
    <button class="primary" onclick={save} disabled={busy}>Save period</button>
  {/snippet}
</Modal>
