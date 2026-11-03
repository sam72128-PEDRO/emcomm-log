<script lang="ts">
  import { RadioTower, FolderOpen, CircleCheck, CircleAlert, TriangleAlert } from "@lucide/svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import type { FolderCheck, GlobalSettings, LogbookType, ProfileInput } from "../lib/types";
  import { TYPE_LABEL } from "../lib/entryForm";
  import { localZoneName, offsetLabel, localOffsetMinutes } from "../lib/time";

  const steps = ["Operator", "Time", "Storage", "Backups", "Presets", "Finish"];
  let step = $state(0);
  let error = $state<string | null>(null);
  let fieldErrors = $state<Record<string, string>>({});
  let busy = $state(false);

  let profile = $state<ProfileInput>({
    name: "",
    callsign: "",
    license_class: "",
    organization: "",
    position: "",
    grid: "",
  });
  const base = app.settings!;
  let s = $state<GlobalSettings>(JSON.parse(JSON.stringify(base)));
  let liveCheck = $state<FolderCheck | null>(null);
  let backupCheck = $state<FolderCheck | null>(null);
  let skipBackups = $state(false);
  let createdProfileId: string | null = null;

  const types: LogbookType[] = ["general", "ham", "emergency"];
  for (const t of types) {
    if (!s.default_preset_ids[t]) {
      const p = app.presets.find((p) => p.logbook_type === t);
      if (p) s.default_preset_ids[t] = p.id;
    }
  }

  async function chooseLive() {
    const p = await api.pickFolder("Choose a folder for your logbooks", s.default_logbook_parent);
    if (!p) return;
    s.default_logbook_parent = p;
    liveCheck = await api.checkFolder(p, "live");
  }
  async function chooseBackup() {
    const p = await api.pickFolder(
      "Choose an independent backup folder (another drive is best)",
      s.backup.destination,
    );
    if (!p) return;
    s.backup.destination = p;
    backupCheck = await api.checkFolder(p, "backup");
    if (
      backupCheck.ok &&
      s.default_logbook_parent &&
      (p === s.default_logbook_parent ||
        p.startsWith(s.default_logbook_parent + "/") ||
        p.startsWith(s.default_logbook_parent + "\\"))
    ) {
      backupCheck = {
        ...backupCheck,
        message:
          "This is inside your logbook folder. A backup on the same drive does not protect against drive failure; prefer another drive.",
      };
    }
  }

  function validateStep(): boolean {
    fieldErrors = {};
    error = null;
    if (step === 0) {
      if (!profile.name.trim()) fieldErrors.name = "Operator name is required";
      if (profile.callsign && !/^[A-Za-z0-9]+(\/[A-Za-z0-9]+)*$/.test(profile.callsign.trim()))
        fieldErrors.callsign = "Letters, digits and / only";
    }
    if (step === 2) {
      if (!s.default_logbook_parent) fieldErrors.live = "Choose a folder";
      else if (liveCheck && !liveCheck.ok) fieldErrors.live = liveCheck.message ?? "Folder not usable";
    }
    if (step === 3) {
      if (!skipBackups) {
        if (!s.backup.destination)
          fieldErrors.backup = "Choose a backup folder, or explicitly skip backups below";
        else if (backupCheck && !backupCheck.ok)
          fieldErrors.backup = backupCheck.message ?? "Folder not usable";
      }
      if (s.backup.interval_minutes < 5 || s.backup.interval_minutes > 1440)
        fieldErrors.interval = "5–1440 minutes";
      if (s.backup.keep_recent < 1) fieldErrors.retention = "Keep at least 1 recent backup";
    }
    const first = Object.keys(fieldErrors)[0];
    if (first) {
      app.announce(fieldErrors[first], true);
      document.getElementById(`wz-${first}`)?.focus();
      return false;
    }
    return true;
  }

  function next() {
    if (validateStep()) step = Math.min(step + 1, steps.length - 1);
  }

  async function finish() {
    busy = true;
    error = null;
    try {
      const p = createdProfileId
        ? await api.updateProfile(createdProfileId, profile)
        : await api.createProfile(profile);
      createdProfileId = p.id;
      const toSave: GlobalSettings = {
        ...s,
        backup: { ...s.backup, destination: skipBackups ? null : s.backup.destination },
        active_profile_id: p.id,
        wizard_completed: true,
      };
      const saved = await api.saveSettings(toSave);
      app.settings = saved;
      await app.refreshProfilesPresets();
      app.toast("ok", "Setup saved. Create your first logbook to start logging.");
      app.screen = "home";
    } catch (e) {
      const err = toAppError(e);
      error = `Settings could not be saved: ${err.message}`;
      app.announce(error, true);
    } finally {
      busy = false;
    }
  }
</script>

<main class="wizard">
  <div class="panel card">
    <header class="head">
      <RadioTower size={34} aria-hidden="true" />
      <div>
        <h1>Welcome to EMCOMM LOG</h1>
        <p class="muted" style="margin:2px 0 0">
          First-run setup. Everything stays on this computer; no network is used.
        </p>
      </div>
    </header>
    <ol class="steps" aria-label="Setup steps">
      {#each steps as label, i (label)}
        <li aria-current={i === step ? "step" : undefined} class:done={i < step}>{i + 1}. {label}</li>
      {/each}
    </ol>

    <section class="body" aria-live="polite">
      {#if step === 0}
        <h2>Operator profile</h2>
        <p class="muted">
          Your profile is stored only on this computer. Callsign and license class can stay blank if you don't
          operate amateur radio.
        </p>
        <div class="grid-2">
          <div class="field">
            <label for="wz-name">Operator name<span class="req">*</span></label>
            <input
              id="wz-name"
              type="text"
              bind:value={profile.name}
              aria-invalid={fieldErrors.name ? "true" : undefined}
              aria-describedby="wz-name-err"
            />
            {#if fieldErrors.name}<span class="field-error" id="wz-name-err">{fieldErrors.name}</span>{/if}
          </div>
          <div class="field">
            <label for="wz-callsign">Callsign (optional)</label>
            <input
              id="wz-callsign"
              type="text"
              bind:value={profile.callsign}
              autocapitalize="characters"
              aria-invalid={fieldErrors.callsign ? "true" : undefined}
            />
            {#if fieldErrors.callsign}<span class="field-error">{fieldErrors.callsign}</span>{/if}
          </div>
          <div class="field">
            <label for="wz-lic">License class (optional)</label>
            <input id="wz-lic" type="text" bind:value={profile.license_class} />
          </div>
          <div class="field">
            <label for="wz-grid">Grid locator (optional)</label>
            <input id="wz-grid" type="text" bind:value={profile.grid} placeholder="e.g. EM73" />
          </div>
          <div class="field">
            <label for="wz-org">Organization / unit (optional)</label>
            <input id="wz-org" type="text" bind:value={profile.organization} />
          </div>
          <div class="field">
            <label for="wz-pos">Position (optional)</label>
            <input id="wz-pos" type="text" bind:value={profile.position} placeholder="e.g. Comm Unit" />
          </div>
        </div>
      {:else if step === 1}
        <h2>Time display</h2>
        <p class="muted">
          All times are stored in UTC. This only controls how they are shown and entered. Your computer's zone
          is {localZoneName()} ({offsetLabel(localOffsetMinutes())}).
        </p>
        <fieldset>
          <legend>Show and enter times in</legend>
          <label class="check"
            ><input type="radio" name="tz" value="local" bind:group={s.time_display} /> Local time</label
          >
          <label class="check"
            ><input type="radio" name="tz" value="utc" bind:group={s.time_display} /> UTC (Zulu)</label
          >
        </fieldset>
        <fieldset>
          <legend>Clock</legend>
          <label class="check"
            ><input type="radio" name="clk" value="24h" bind:group={s.clock} /> 24-hour</label
          >
          <label class="check"
            ><input type="radio" name="clk" value="12h" bind:group={s.clock} /> 12-hour</label
          >
        </fieldset>
        <fieldset>
          <legend>Theme</legend>
          <label class="check"><input type="radio" name="th" value="dark" bind:group={s.theme} /> Dark</label>
          <label class="check"
            ><input type="radio" name="th" value="light" bind:group={s.theme} /> Light</label
          >
          <label class="check"
            ><input type="radio" name="th" value="system" bind:group={s.theme} /> Follow system</label
          >
        </fieldset>
      {:else if step === 2}
        <h2>Where logbooks are stored</h2>
        <p class="muted">
          Each logbook is a folder containing its database and attachments. Every saved entry is written
          directly to it. Use a local disk or a directly attached drive — not a network share or a
          cloud-synced folder.
        </p>
        <div class="field">
          <label for="wz-live">Default logbook folder<span class="req">*</span></label>
          <div class="row">
            <input
              id="wz-live"
              type="text"
              readonly
              value={s.default_logbook_parent ?? ""}
              placeholder="No folder chosen"
              aria-invalid={fieldErrors.live ? "true" : undefined}
            />
            <button onclick={chooseLive}><FolderOpen size={16} />Browse…</button>
          </div>
          {#if liveCheck}
            <span class={liveCheck.ok ? "hint" : "field-error"}
              >{#if liveCheck.ok}<CircleCheck size={14} /> Writable{#if liveCheck.message}
                  — {liveCheck.message}{/if}{:else}<CircleAlert size={14} /> {liveCheck.message}{/if}</span
            >
          {/if}
          {#if fieldErrors.live}<span class="field-error">{fieldErrors.live}</span>{/if}
        </div>
      {:else if step === 3}
        <h2>Backups</h2>
        <p class="muted">
          Backups are independent, verified copies made every interval while a logbook is open, when you close
          it, and on demand. The backup folder must be outside the live logbook — another drive is best.
        </p>
        <div class="field">
          <label for="wz-backup">Backup folder</label>
          <div class="row">
            <input
              id="wz-backup"
              type="text"
              readonly
              value={s.backup.destination ?? ""}
              placeholder="No folder chosen"
              disabled={skipBackups}
              aria-invalid={fieldErrors.backup ? "true" : undefined}
            />
            <button onclick={chooseBackup} disabled={skipBackups}><FolderOpen size={16} />Browse…</button>
          </div>
          {#if backupCheck && !skipBackups}
            <span class={backupCheck.ok ? "hint" : "field-error"}
              >{#if backupCheck.ok}<CircleCheck size={14} /> Writable{#if backupCheck.message}
                  — {backupCheck.message}{/if}{:else}<CircleAlert size={14} />
                {backupCheck.message}{/if}</span
            >
          {/if}
          {#if fieldErrors.backup}<span class="field-error">{fieldErrors.backup}</span>{/if}
        </div>
        <div class="grid-4">
          <div class="field">
            <label for="wz-interval">Every (minutes)</label>
            <input id="wz-interval" type="number" min="5" max="1440" bind:value={s.backup.interval_minutes} />
            {#if fieldErrors.interval}<span class="field-error">{fieldErrors.interval}</span>{/if}
          </div>
          <div class="field">
            <label for="wz-retention">Keep most recent</label>
            <input id="wz-retention" type="number" min="1" max="500" bind:value={s.backup.keep_recent} />
          </div>
          <div class="field">
            <label for="wz-daily">Daily copies</label>
            <input id="wz-daily" type="number" min="0" max="365" bind:value={s.backup.keep_daily} />
          </div>
          <div class="field">
            <label for="wz-weekly">Weekly copies</label>
            <input id="wz-weekly" type="number" min="0" max="520" bind:value={s.backup.keep_weekly} />
          </div>
        </div>
        {#if fieldErrors.retention}<span class="field-error">{fieldErrors.retention}</span>{/if}
        <label class="check"
          ><input type="checkbox" bind:checked={s.backup.on_close} /> Also back up when a logbook is closed</label
        >
        <label class="check"><input type="checkbox" bind:checked={skipBackups} /> Skip backups for now</label>
        {#if skipBackups}
          <div class="banner warn" role="alert">
            <TriangleAlert size={18} class="icon" />
            <div>
              Without a backup destination, no independent copy of your logbooks will be made. Backup status
              will show <strong>Not configured</strong> until you choose a folder in Settings → Backups. Logging
              still works.
            </div>
          </div>
        {/if}
      {:else if step === 4}
        <h2>Starting presets</h2>
        <p class="muted">
          A preset supplies defaults, quick tags and starter templates when you create a logbook. The logbook
          keeps its own copy, so later preset edits never change existing logbooks.
        </p>
        {#each types as t (t)}
          {@const opts = app.presets.filter((p) => p.logbook_type === t)}
          {@const sel = opts.find((p) => p.id === s.default_preset_ids[t])}
          <div class="field">
            <label for="wz-preset-{t}">{TYPE_LABEL[t]} logbooks</label>
            <select id="wz-preset-{t}" bind:value={s.default_preset_ids[t]}>
              {#each opts as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
            </select>
            {#if sel}<span class="hint"
                >{sel.config.templates.length} template(s){#if sel.config.templates.length}: {sel.config.templates
                    .map((x) => x.name)
                    .join(", ")}{/if}; quick tags {sel.config.quick_tags.join(", ") || "none"}</span
              >{/if}
          </div>
        {/each}
      {:else}
        <h2>Review</h2>
        <dl class="review">
          <dt>Operator</dt>
          <dd>{profile.name}{profile.callsign ? ` (${profile.callsign.toUpperCase()})` : ""}</dd>
          <dt>Times</dt>
          <dd>{s.time_display === "utc" ? "UTC" : "Local"}, {s.clock === "24h" ? "24-hour" : "12-hour"}</dd>
          <dt>Logbook folder</dt>
          <dd class="mono">{s.default_logbook_parent}</dd>
          <dt>Backups</dt>
          <dd>
            {#if skipBackups || !s.backup.destination}<span class="badge warn">Not configured</span
              >{:else}<span class="mono">{s.backup.destination}</span>, every {s.backup.interval_minutes} min{s
                .backup.on_close
                ? " and on close"
                : ""}; keep {s.backup.keep_recent} recent, {s.backup.keep_daily} daily, {s.backup.keep_weekly} weekly{/if}
          </dd>
        </dl>
        <p class="muted">Settings are stored at <span class="mono">{app.dataDir}</span>.</p>
      {/if}
      {#if error}<div class="banner error" role="alert">
          <CircleAlert size={18} class="icon" />{error}
        </div>{/if}
    </section>

    <footer class="foot">
      <button onclick={() => (step = Math.max(0, step - 1))} disabled={step === 0 || busy}>Back</button>
      <span class="spacer"></span>
      {#if step < steps.length - 1}
        <button class="primary" onclick={next}>Continue</button>
      {:else}
        <button class="primary" onclick={finish} disabled={busy}>{busy ? "Saving…" : "Finish setup"}</button>
      {/if}
    </footer>
  </div>
</main>

<style>
  .wizard {
    min-height: 100vh;
    display: grid;
    place-items: center;
    padding: 24px;
  }
  .card {
    width: 760px;
    max-width: 100%;
  }
  .head {
    display: flex;
    gap: 14px;
    align-items: center;
    padding: 18px 22px;
    border-bottom: 1px solid var(--border);
    color: var(--cyan);
  }
  .head h1 {
    color: var(--text);
  }
  .steps {
    display: flex;
    gap: 6px;
    list-style: none;
    margin: 0;
    padding: 12px 22px;
    flex-wrap: wrap;
    border-bottom: 1px solid var(--border);
  }
  .steps li {
    padding: 4px 10px;
    border-radius: 6px;
    color: var(--muted);
    font-size: 13px;
  }
  .steps li.done {
    color: var(--ok);
  }
  .steps li[aria-current="step"] {
    background: var(--fill);
    color: #fff;
    font-weight: 600;
  }
  .body {
    padding: 18px 22px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    min-height: 320px;
  }
  .body p {
    margin: 0;
  }
  fieldset {
    display: flex;
    gap: 22px;
    flex-wrap: wrap;
  }
  .foot {
    display: flex;
    padding: 14px 22px;
    border-top: 1px solid var(--border);
  }
  .review {
    display: grid;
    grid-template-columns: 160px 1fr;
    gap: 8px 14px;
    margin: 0;
  }
  .review dt {
    color: var(--muted);
  }
  .review dd {
    margin: 0;
    word-break: break-all;
  }
</style>
