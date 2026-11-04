<script lang="ts">
  import { Crosshair, CircleAlert, CircleCheck, TriangleAlert, LoaderCircle } from "@lucide/svelte";
  import { onDestroy } from "svelte";
  import Modal from "./Modal.svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import { DEFAULT_GPS, GPS_MAX_AGE_S, GPS_TIMEOUT_S, STATUS_TEXT, preferredRequest } from "../lib/gps";
  import { describeSource, formatCoord } from "../lib/location";
  import { formatInstant } from "../lib/time";
  import type * as T from "../lib/types";

  let {
    onaccept,
    onclose,
    request = null,
  }: {
    /** Absent = test mode (Settings → Test GPS): nothing is attached. */
    onaccept?: (l: T.Location) => void;
    onclose: () => void;
    /** Use this exact source (Settings test) instead of the preferred one. */
    request?: T.GpsRequest | null;
  } = $props();

  let sources = $state<T.GpsSource[]>([]);
  let choice = $state("");
  let running = $state(false);
  let elapsed = $state(0);
  let outcome = $state<T.GpsOutcome | null>(null);
  let error = $state<string | null>(null);
  let timer: ReturnType<typeof setInterval> | null = null;

  const key = (r: Pick<T.GpsRequest, "provider" | "path">) => `${r.provider}|${r.path}`;

  function requestFor(k: string): T.GpsRequest | null {
    const g = { ...DEFAULT_GPS, ...(app.settings?.gps ?? {}) };
    const [provider, path] = k.split("|") as [T.GpsRequest["provider"], string];
    if (!provider) return null;
    const src = sources.find((s) => key(s) === k);
    return {
      provider,
      path,
      baud: provider === "serial" && path === g.serial_path ? g.baud : 0,
      serial_kind: (src?.serial_kind ?? g.serial_kind) as T.SerialKind,
      gpsd_endpoint: g.gpsd_endpoint,
      timeout_s: GPS_TIMEOUT_S,
      max_age_s: GPS_MAX_AGE_S,
    };
  }

  async function start() {
    const req = request ?? requestFor(choice);
    if (!req) {
      error = "No GPS source is set up. Choose one, or close and enter the position by hand.";
      return;
    }
    running = true;
    outcome = null;
    error = null;
    elapsed = 0;
    timer = setInterval(() => (elapsed += 1), 1000);
    try {
      outcome = await api.gpsAcquire(req);
      app.announce(`GPS: ${STATUS_TEXT[outcome.status]}`, outcome.status !== "fix");
    } catch (e) {
      error = toAppError(e).message;
    } finally {
      running = false;
      if (timer) clearInterval(timer);
    }
  }

  async function cancel() {
    await api.gpsCancel().catch(() => {});
  }

  function close() {
    if (running) void cancel();
    onclose();
  }

  onDestroy(() => {
    if (timer) clearInterval(timer);
    if (running) void api.gpsCancel();
  });

  api
    .gpsSources()
    .then((s) => {
      sources = s;
      const pref = request ?? preferredRequest(app.settings?.gps, s);
      choice = pref ? key(pref) : s[0] ? key(s[0]) : "";
      // Opening Capture GPS is the explicit request for one fix.
      if (choice || request) void start();
    })
    .catch((e) => (error = toAppError(e).message));

  const loc = $derived(outcome?.location ?? null);
</script>

<Modal title={onaccept ? "Capture GPS position" : "Test GPS"} onclose={close} busy={false}>
  {#if !request}
    <div class="row wrap">
      <label for="gps-src">Source</label>
      <select id="gps-src" bind:value={choice} disabled={running} style="max-width:360px">
        {#each sources as s (key(s))}<option value={key(s)}
            >{s.label}{s.detail ? ` — ${s.detail}` : ""}</option
          >{/each}
        {#if !sources.length}<option value="">No GPS sources found</option>{/if}
      </select>
      <button class="small" onclick={start} disabled={running || !choice}
        ><Crosshair size={14} />Get fix</button
      >
    </div>
  {/if}

  {#if running}
    <div class="banner info" aria-live="polite">
      <LoaderCircle size={18} class="icon spin" />
      <div class="grow">
        Acquiring a fix… {elapsed} s of up to {GPS_TIMEOUT_S} s. A receiver that has just been switched on can take
        longer outdoors.
      </div>
      <button class="small" onclick={cancel}>Cancel</button>
    </div>
  {/if}

  {#if error}<div class="banner error" role="alert">
      <CircleAlert size={18} class="icon" />
      <div>{error}</div>
    </div>{/if}

  {#if outcome && !running}
    {#if outcome.status === "fix" || outcome.status === "stale_fix"}
      <div class="banner {outcome.status === 'fix' ? 'ok' : 'warn'}" role="status">
        {#if outcome.status === "fix"}<CircleCheck size={18} class="icon" />{:else}<TriangleAlert
            size={18}
            class="icon"
          />{/if}
        <div class="grow">
          <strong>{STATUS_TEXT[outcome.status]}</strong>{outcome.status === "stale_fix"
            ? ` — ${outcome.message}`
            : ""}
          {#if loc}
            <dl class="kv">
              <dt>Position</dt>
              <dd class="mono">
                {formatCoord(loc.latitude_deg, loc.longitude_deg)}{loc.maidenhead
                  ? ` · ${loc.maidenhead}`
                  : ""}
              </dd>
              <dt>Source</dt>
              <dd>{describeSource(loc.source)}<span class="muted"> · {loc.provider_id}</span></dd>
              <dt>Fix time</dt>
              <dd>
                {loc.captured_at_utc
                  ? formatInstant(loc.captured_at_utc, app.zone, app.clock)
                  : "—"}{outcome.fix_age_s != null ? ` (${Math.round(outcome.fix_age_s)} s ago)` : ""}
              </dd>
              <dt>Quality</dt>
              <dd>
                {loc.accuracy_m != null
                  ? `±${Math.round(loc.accuracy_m)} m (reported)`
                  : "accuracy unknown"}{loc.satellites != null
                  ? ` · ${loc.satellites} satellites`
                  : ""}{loc.altitude_m != null ? ` · altitude ${Math.round(loc.altitude_m)} m` : ""}
              </dd>
            </dl>
          {/if}
        </div>
      </div>
    {:else}
      <div class="banner error" role="alert">
        <CircleAlert size={18} class="icon" />
        <div>
          <strong>{STATUS_TEXT[outcome.status]}.</strong>
          {outcome.message}
          {#if outcome.sentences || outcome.rejected}<div class="muted small">
              {outcome.sentences} valid sentences, {outcome.rejected} rejected{outcome.baud
                ? ` at ${outcome.baud} baud`
                : ""}
            </div>{/if}
          <div class="muted small">You can still save the entry without a position, or enter it by hand.</div>
        </div>
      </div>
    {/if}
  {/if}

  {#snippet footer()}
    <button onclick={close}>{onaccept ? "Cancel" : "Close"}</button>
    {#if outcome && !running && !["fix", "stale_fix"].includes(outcome.status)}
      <button onclick={start}>Retry</button>
    {/if}
    {#if onaccept && loc && !running}
      <button
        class={outcome?.status === "fix" ? "primary" : ""}
        onclick={() => {
          onaccept!(loc!);
        }}>{outcome?.status === "fix" ? "Use this position" : "Use this old position anyway"}</button
      >
    {/if}
  {/snippet}
</Modal>

<style>
  .kv {
    margin: 6px 0 0;
  }
  .small {
    font-size: 12.5px;
  }
</style>
