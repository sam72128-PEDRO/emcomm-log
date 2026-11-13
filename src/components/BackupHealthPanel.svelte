<script lang="ts">
  import { CircleCheck, CircleAlert, CircleHelp, LoaderCircle, ShieldCheck, RefreshCw } from "@lucide/svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import { formatInstant } from "../lib/time";
  import type * as T from "../lib/types";

  /** 2.3 Backup Health: read-only status over the existing backup engine, plus Verify Latest Backup. */
  let { disabled = false }: { disabled?: boolean } = $props();

  let verifying = $state(false);
  let report = $state<T.VerifyReport | null>(null);
  let error = $state<string | null>(null);
  const h = $derived(app.backupHealth);

  const tone = (st: T.BackupHealthState) =>
    st === "verified" ? "ok" : st === "unknown" ? "neutral" : st === "attention_required" ? "warn" : "danger";
  const when = (t: string | null | undefined) => (t ? formatInstant(t, app.zone, app.clock) : "—");
  function age(min: number | null) {
    if (min === null) return "—";
    if (min < 60) return `${min} min`;
    if (min < 48 * 60) return `${Math.floor(min / 60)} h ${min % 60} min`;
    return `${Math.floor(min / 1440)} days`;
  }

  async function verify() {
    verifying = true;
    error = null;
    report = null;
    try {
      report = await api.verifyLatestBackup();
      if (report.complete) app.toast("ok", "Latest backup verified");
      else app.toast("error", "Latest backup failed verification");
    } catch (e) {
      error = toAppError(e).message;
    } finally {
      verifying = false;
      await app.refreshBackupHealth();
    }
  }
</script>

<fieldset class="health" aria-live="polite">
  <legend><ShieldCheck size={14} /> Backup Health</legend>
  {#if !h}
    <p class="muted">Backup health is not available.</p>
  {:else}
    <div class="row wrap">
      <span class="badge {tone(h.state)}">{h.state_label.toUpperCase()}</span>
      <strong>{h.headline}</strong>
      <span class="spacer"></span>
      <button
        class="small ghost icon-only"
        aria-label="Refresh backup health"
        title="Refresh"
        onclick={() => app.refreshBackupHealth()}><RefreshCw size={14} /></button
      >
    </div>
    {#if h.reasons.length > 1}<ul class="reasons">
        {#each h.reasons.slice(1) as r (r)}<li>{r}</li>{/each}
      </ul>{/if}
    <dl class="kv">
      <dt>Destination</dt>
      <dd>
        {#if h.destination}<span class="mono">{h.destination}</span>
          {#if h.destination_accessible === false}<span class="badge danger">NOT ACCESSIBLE</span
            >{:else if h.destination_accessible}<span class="badge ok">ACCESSIBLE</span>{/if}
          {#if h.destination_message}<div class="field-error small">{h.destination_message}</div>{/if}
        {:else}Not configured{/if}
      </dd>
      <dt>Schedule</dt>
      <dd>
        {#if h.destination}Every {h.interval_minutes} min while a logbook is open{h.on_close
            ? ", and on close"
            : ""}{:else}—{/if}
      </dd>
      <dt>Last attempted backup</dt>
      <dd>
        {#if h.last_attempt}{when(h.last_attempt.finished_at_utc)} ({h.last_attempt.kind}) —
          {#if h.last_attempt.ok}<span class="ok-text">succeeded</span>{:else}<span class="danger-text"
              >failed: {h.last_attempt.message}</span
            >{/if}{:else}None recorded on this computer{/if}
      </dd>
      <dt>Last successful backup</dt>
      <dd>
        {#if h.latest_backup}{when(h.latest_backup.created_at_utc)} ({h.latest_backup.kind}, {h.latest_backup
            .entry_count} entries) <span class="muted">— checked when created</span>{:else}None at the
          destination{/if}
      </dd>
      <dt>Backup age</dt>
      <dd>{age(h.backup_age_minutes)}</dd>
      <dt>Last verified backup</dt>
      <dd>
        {#if h.latest_verification}{when(h.latest_verification.verified_at_utc)} —
          {#if h.latest_verification.complete}<span class="ok-text">passed</span>{:else}<span
              class="danger-text">failed</span
            >{/if}
        {:else if h.last_verification_any}Latest backup not yet verified (an older one was verified {when(
            h.last_verification_any.verified_at_utc,
          )}){:else}Never verified{/if}
      </dd>
      <dt>Protection</dt>
      <dd>
        {h.protection}
        {#if h.last_unchanged_check_utc}<div class="muted small">
            Last scheduled check {when(h.last_unchanged_check_utc)}: no changes, so no new backup was needed.
          </div>{/if}
      </dd>
      {#if h.last_error}<dt>Last recorded error</dt>
        <dd class="danger-text">{h.last_error}</dd>{/if}
    </dl>
    <div class="row wrap">
      <button
        onclick={verify}
        disabled={disabled || verifying || !h.latest_backup || h.state === "unavailable"}
      >
        {#if verifying}<LoaderCircle size={14} class="spin" />Verifying…{:else}Verify Latest Backup{/if}
      </button>
      <span class="hint"
        >Reads every file of the newest backup; it never changes, prunes, restores or repairs it.</span
      >
    </div>
  {/if}
  {#if error}<div class="banner error" role="alert"><CircleAlert size={16} class="icon" />{error}</div>{/if}
  {#if report}
    <div class="report">
      <p>
        {#if report.complete}<CircleCheck size={15} class="ok-text" />{:else}<CircleAlert
            size={15}
            class="danger-text"
          />{/if}
        <strong>{report.scope}</strong>
      </p>
      <p class="muted small mono">{report.path}</p>
      <ul class="steps">
        {#each report.steps as st (st.id)}
          <li>
            {#if st.status === "passed"}<CircleCheck
                size={14}
                class="ok-text"
              />{:else if st.status === "failed"}<CircleAlert
                size={14}
                class="danger-text"
              />{:else}<CircleHelp size={14} class="muted" />{/if}
            <span>{st.label}</span>
            <span class="muted"
              >{st.status === "not_run" ? "not checked (an earlier step failed)" : st.detail}</span
            >
          </li>
        {/each}
      </ul>
      {#if report.problems.length}<details open>
          <summary>{report.problems.length} problem(s)</summary>
          <ul>
            {#each report.problems as p, i (i)}<li>{p}</li>{/each}
          </ul>
        </details>{/if}
    </div>
  {/if}
</fieldset>

<style>
  .health {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .reasons {
    margin: 0;
    padding-left: 18px;
  }
  .steps {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .steps li {
    display: flex;
    gap: 8px;
    align-items: baseline;
  }
  .report p {
    margin: 0;
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .ok-text {
    color: var(--ok, #2e7d32);
  }
  .danger-text {
    color: var(--danger, #c62828);
  }
</style>
