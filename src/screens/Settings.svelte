<script lang="ts">
  import GpsCaptureDialog from "../components/GpsCaptureDialog.svelte";
  import { DEFAULT_GPS, GPS_MAX_AGE_S, GPS_TIMEOUT_S } from "../lib/gps";
  import { IS_ANDROID, DEVICE } from "../lib/platform";
  import {
    Settings as SettingsIcon,
    FolderOpen,
    CircleAlert,
    CircleCheck,
    UserPlus,
    Plus,
    FolderSearch,
    RefreshCw,
    Copy,
    Trash2,
    PencilLine,
  } from "@lucide/svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import { backupNow } from "../lib/actions";
  import { confirm } from "../lib/confirm.svelte";
  import { restoreDraft } from "../lib/recovery";
  import type * as T from "../lib/types";
  import { TYPE_LABEL, KIND_LABEL, DIRECTION_LABEL, PRIORITY_LABEL, allowedKinds } from "../lib/entryForm";
  import { formatInstant, relativeAge } from "../lib/time";
  import Modal from "../components/Modal.svelte";
  import TagInput from "../components/TagInput.svelte";

  type Section = "general" | "profiles" | "presets" | "backups" | "location" | "storage";
  let section = $state<Section>("general");
  const sections: { id: Section; label: string }[] = [
    { id: "general", label: "General" },
    { id: "profiles", label: "Operator Profiles" },
    { id: "presets", label: "Presets" },
    { id: "backups", label: "Backups" },
    { id: "location", label: "Location & GPS" },
    { id: "storage", label: "Storage & Recovery" },
  ];

  const copySettings = (): T.GlobalSettings => {
    const c: T.GlobalSettings = JSON.parse(JSON.stringify(app.settings));
    c.gps ??= { ...DEFAULT_GPS };
    return c;
  };
  let s = $state<T.GlobalSettings>(copySettings());
  let gpsSources = $state<T.GpsSource[] | null>(null);
  let gpsTest = $state<T.GpsRequest | null>(null);
  async function detectReceivers() {
    gpsSources = null;
    gpsSources = await api.gpsSources().catch(() => []);
  }
  function testRequest(): T.GpsRequest | null {
    const g = s.gps!;
    const base = {
      baud: g.baud,
      serial_kind: g.serial_kind,
      gpsd_endpoint: g.gpsd_endpoint,
      timeout_s: GPS_TIMEOUT_S,
      max_age_s: GPS_MAX_AGE_S,
    };
    if (g.preferred === "serial" || (g.preferred === "auto" && g.serial_path))
      return { ...base, provider: "serial", path: g.serial_path };
    if (g.preferred === "gpsd") return { ...base, provider: "gpsd", path: "" };
    if (g.preferred === "os" || g.preferred === "auto") return { ...base, provider: "os", path: "" };
    return null;
  }
  let saving = $state(false);
  let saveErr = $state<string | null>(null);
  let liveCheck = $state<T.FolderCheck | null>(null);
  let backupCheck = $state<T.FolderCheck | null>(null);
  const dirty = $derived(JSON.stringify(s) !== JSON.stringify(app.settings));

  async function saveSettings() {
    saving = true;
    saveErr = null;
    try {
      app.settings = await api.saveSettings(s);
      s = JSON.parse(JSON.stringify(app.settings));
      app.toast("ok", "Settings saved");
      if (app.active) await app.refreshActive();
    } catch (e) {
      const err = toAppError(e);
      saveErr = err.message;
      app.announce(`Settings not saved: ${err.message}`, true);
    } finally {
      saving = false;
    }
  }
  function revert() {
    s = copySettings();
    saveErr = null;
  }

  async function chooseLive() {
    const p = await api.pickFolder("Default folder for new logbooks", s.default_logbook_parent);
    if (!p) return;
    s.default_logbook_parent = p;
    liveCheck = await api.checkFolder(p, "live");
  }
  async function chooseBackup() {
    const p = await api.pickFolder("Backup destination (independent of live logbooks)", s.backup.destination);
    if (!p) return;
    s.backup.destination = p;
    backupCheck = await api.checkFolder(p, "backup");
  }

  // ----- per-logbook override -----
  let tzOverride = $state<string>(app.active?.settings.time_display_override ?? "");
  async function saveOverride() {
    try {
      const st = await api.updateLogbookSettings(tzOverride || null, app.active!.settings.operational_notes);
      if (app.active) app.active.settings = st;
      app.toast("ok", "Logbook time display saved in the logbook");
    } catch (e) {
      app.toast("error", toAppError(e).message);
    }
  }

  // ----- profiles -----
  let profileEdit = $state<{ id: string | null; input: T.ProfileInput } | null>(null);
  let profileErr = $state<string | null>(null);
  async function saveProfile() {
    if (!profileEdit) return;
    profileErr = null;
    try {
      if (profileEdit.id) await api.updateProfile(profileEdit.id, profileEdit.input);
      else await api.createProfile(profileEdit.input);
      await app.refreshProfilesPresets();
      profileEdit = null;
      app.toast("ok", "Profile saved. Existing logbooks and entries keep their original operator snapshot.");
    } catch (e) {
      profileErr = toAppError(e).message;
    }
  }
  async function makeActive(p: T.Profile) {
    try {
      app.settings = await api.saveSettings({ ...app.settings!, active_profile_id: p.id });
      s.active_profile_id = p.id;
      app.toast("ok", `${p.name} is now the active operator`);
    } catch (e) {
      app.toast("error", toAppError(e).message);
    }
  }
  async function toggleProfile(p: T.Profile) {
    try {
      await api.setProfileActive(p.id, !p.active);
      await app.refreshProfilesPresets();
    } catch (e) {
      app.toast("error", toAppError(e).message);
    }
  }

  // ----- presets -----
  let presetEdit = $state<{ id: string | null; input: T.PresetInput } | null>(null);
  let presetErr = $state<string | null>(null);
  const HIDEABLE = [
    "channel",
    "location",
    "tags",
    "attachments",
    "handling",
    "flagged",
    "to_text",
    "from_text",
  ];
  const LABELABLE = ["from_text", "to_text", "subject", "body", "channel", "location", "tags"];
  const blankConfig = (): T.PresetConfig => ({
    default_kind: null,
    default_direction: null,
    default_priority: null,
    default_channel: "",
    default_mode: "",
    hidden_fields: [],
    field_labels: {},
    quick_tags: [],
    channels: [],
    templates: [],
  });
  function newPreset(t: T.LogbookType) {
    presetErr = null;
    presetEdit = { id: null, input: { name: "", logbook_type: t, profile_id: null, config: blankConfig() } };
  }
  function editPreset(p: T.Preset) {
    presetErr = null;
    presetEdit = {
      id: p.id,
      input: {
        name: p.name,
        logbook_type: p.logbook_type,
        profile_id: p.profile_id,
        config: JSON.parse(JSON.stringify(p.config)),
      },
    };
  }
  async function savePreset() {
    if (!presetEdit) return;
    presetErr = null;
    try {
      if (presetEdit.id) await api.updatePreset(presetEdit.id, presetEdit.input);
      else await api.createPreset(presetEdit.input);
      await app.refreshProfilesPresets();
      presetEdit = null;
      app.toast("ok", "Preset saved. Existing logbooks are not changed.");
    } catch (e) {
      presetErr = toAppError(e).message;
    }
  }
  async function duplicatePreset(p: T.Preset) {
    try {
      await api.duplicatePreset(p.id);
      await app.refreshProfilesPresets();
    } catch (e) {
      app.toast("error", toAppError(e).message);
    }
  }
  async function deletePreset(p: T.Preset) {
    const a = await confirm.ask({
      title: "Delete preset?",
      message: `"${p.name}" will be deleted. Logbooks keep their own copy of any preset they were created from.`,
      buttons: [
        { id: "del", label: "Delete", tone: "danger" },
        { id: "cancel", label: "Cancel", tone: "primary" },
      ],
      cancelId: "cancel",
    });
    if (a.id !== "del") return;
    try {
      await api.deletePreset(p.id);
      await app.refreshProfilesPresets();
    } catch (e) {
      app.toast("error", toAppError(e).message);
    }
  }
  async function copyTemplatesFromLogbook() {
    if (!presetEdit || !app.active) return;
    if (app.active.info.logbook_type !== presetEdit.input.logbook_type) {
      presetErr = "The open logbook is a different type than this preset";
      return;
    }
    const ts = await api.listTemplates();
    presetEdit.input.config.templates = ts.map((t) => ({ name: t.name, defaults: t.defaults }));
  }
  function toggleHidden(f: string) {
    if (!presetEdit) return;
    const h = presetEdit.input.config.hidden_fields;
    presetEdit.input.config.hidden_fields = h.includes(f) ? h.filter((x) => x !== f) : [...h, f];
  }
  function setLabel(f: string, v: string) {
    if (!presetEdit) return;
    const l = { ...presetEdit.input.config.field_labels };
    if (v.trim()) l[f] = v;
    else delete l[f];
    presetEdit.input.config.field_labels = l;
  }

  // ----- backups list -----
  let backups = $state<T.BackupListing[] | null>(null);
  let backupsErr = $state<string | null>(null);
  async function loadBackups() {
    backupsErr = null;
    try {
      backups = await api.listBackups(null);
    } catch (e) {
      backupsErr = toAppError(e).message;
    }
  }

  // ----- drafts -----
  let drafts = $state<T.Draft[]>([]);
  async function loadDrafts() {
    drafts = app.active ? await api.listDrafts().catch(() => []) : [];
  }
  $effect(() => {
    if (section === "storage") loadDrafts();
    if (section === "backups" && app.settings?.backup.destination) loadBackups();
  });
  async function discardDraft(d: T.Draft) {
    const a = await confirm.ask({
      title: "Discard draft?",
      message:
        "This unfinished entry will be permanently removed from the recovery area. It was never part of the logbook.",
      buttons: [
        { id: "discard", label: "Discard", tone: "danger" },
        { id: "cancel", label: "Keep it", tone: "primary" },
      ],
      cancelId: "cancel",
    });
    if (a.id !== "discard") return;
    await api.deleteDraft(d.logbook_id, d.draft_id).catch(() => {});
    await loadDrafts();
  }
</script>

<section class="panel">
  <div class="panel-head"><h1 class="title"><SettingsIcon size={22} class="icon" />Settings</h1></div>
  <div class="layout">
    <nav class="secnav" aria-label="Settings sections">
      {#each sections as sec (sec.id)}
        <button aria-current={section === sec.id ? "page" : undefined} onclick={() => (section = sec.id)}
          >{sec.label}</button
        >
      {/each}
    </nav>
    <div class="content col">
      {#if section === "general"}
        <h2>General</h2>
        <div class="grid-3">
          <div class="field">
            <label for="st-tz">Show and enter times in</label>
            <select id="st-tz" bind:value={s.time_display}
              ><option value="local">Local time</option><option value="utc">UTC</option></select
            >
          </div>
          <div class="field">
            <label for="st-clock">Clock</label>
            <select id="st-clock" bind:value={s.clock}
              ><option value="24h">24-hour</option><option value="12h">12-hour</option></select
            >
          </div>
          <div class="field">
            <label for="st-theme">Theme</label>
            <select id="st-theme" bind:value={s.theme}
              ><option value="dark">Dark</option><option value="light">Light</option><option value="system"
                >Follow system</option
              ></select
            >
          </div>
        </div>
        <div class="field">
          <label for="st-live">Default folder for new logbooks</label>
          <div class="row">
            <input id="st-live" type="text" readonly value={s.default_logbook_parent ?? ""} /><button
              hidden={IS_ANDROID}
              onclick={chooseLive}><FolderOpen size={16} />Browse…</button
            >
          </div>
          {#if liveCheck}<span class={liveCheck.ok ? "hint" : "field-error"}
              >{liveCheck.ok ? "Writable local folder" : liveCheck.message}</span
            >{/if}
        </div>
        {#if app.active}
          <fieldset>
            <legend>This logbook ({app.active.info.name})</legend>
            <div class="row">
              <div class="field" style="max-width:260px">
                <label for="st-override">Time display for this logbook</label>
                <select id="st-override" bind:value={tzOverride}>
                  <option value="">Use app setting</option><option value="local">Local time</option><option
                    value="utc">UTC</option
                  >
                </select>
              </div>
              <button style="align-self:flex-end" onclick={saveOverride}>Save to logbook</button>
            </div>
            <span class="hint">Stored inside the logbook and carried with .emlog packages.</span>
          </fieldset>
        {/if}
      {:else if section === "profiles"}
        <div class="row">
          <h2>Operator profiles</h2>
          <span class="spacer"></span><button
            onclick={() =>
              (profileEdit = {
                id: null,
                input: {
                  name: "",
                  callsign: "",
                  license_class: "",
                  organization: "",
                  position: "",
                  grid: "",
                },
              })}><UserPlus size={16} />New profile</button
          >
        </div>
        <p class="muted">
          Profiles live only on this {DEVICE}. New entries record a snapshot of the active profile, so editing
          a profile never changes logged history.
        </p>
        <table class="data">
          <thead
            ><tr
              ><th>Name</th><th>Callsign</th><th>License</th><th>Organization / position</th><th>Status</th
              ><th><span class="sr-only">Actions</span></th></tr
            ></thead
          >
          <tbody>
            {#each app.profiles as p (p.id)}
              <tr>
                <td><strong>{p.name}</strong></td>
                <td class="mono">{p.callsign || "—"}</td>
                <td>{p.license_class || "—"}</td>
                <td>{[p.organization, p.position].filter(Boolean).join(" · ") || "—"}</td>
                <td
                  >{#if app.settings?.active_profile_id === p.id}<span class="badge ok">ACTIVE OPERATOR</span
                    >{:else if !p.active}<span class="badge neutral">DEACTIVATED</span>{/if}</td
                >
                <td class="row" style="gap:6px">
                  {#if p.active && app.settings?.active_profile_id !== p.id}<button
                      class="small"
                      onclick={() => makeActive(p)}>Use</button
                    >{/if}
                  <button
                    class="small icon-only"
                    aria-label="Edit {p.name}"
                    onclick={() =>
                      (profileEdit = {
                        id: p.id,
                        input: {
                          name: p.name,
                          callsign: p.callsign,
                          license_class: p.license_class,
                          organization: p.organization,
                          position: p.position,
                          grid: p.grid,
                        },
                      })}><PencilLine size={14} /></button
                  >
                  {#if app.settings?.active_profile_id !== p.id}<button
                      class="small"
                      onclick={() => toggleProfile(p)}>{p.active ? "Deactivate" : "Reactivate"}</button
                    >{/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {:else if section === "presets"}
        <h2>Presets</h2>
        <p class="muted">
          Presets supply defaults, field labels and visibility, quick tags, channels and starter templates. A
          new logbook copies the preset; later edits bump the version and never change existing logbooks.
        </p>
        {#each ["general", "ham", "emergency"] as const as t (t)}
          <div class="row" style="margin-top:6px">
            <h3>{TYPE_LABEL[t]}</h3>
            <span class="spacer"></span><button class="small" onclick={() => newPreset(t)}
              ><Plus size={14} />New {TYPE_LABEL[t]} preset</button
            >
          </div>
          <table class="data">
            <tbody>
              {#each app.presets.filter((p) => p.logbook_type === t) as p (p.id)}
                <tr>
                  <td
                    ><strong>{p.name}</strong> <span class="muted">v{p.version}</span>
                    {#if p.builtin}<span class="badge neutral">BUILT-IN</span>{/if}
                    {#if p.in_use}<span class="badge info">USED</span>{/if}</td
                  >
                  <td
                    >{p.profile_id
                      ? `For ${app.profiles.find((x) => x.id === p.profile_id)?.name ?? "profile"}`
                      : "Any profile"}</td
                  >
                  <td>{p.config.templates.length} template(s)</td>
                  <td class="row" style="gap:6px;justify-content:flex-end">
                    <button
                      class="small icon-only"
                      aria-label="Edit preset {p.name}"
                      onclick={() => editPreset(p)}><PencilLine size={14} /></button
                    >
                    <button
                      class="small icon-only"
                      aria-label="Duplicate preset {p.name}"
                      onclick={() => duplicatePreset(p)}><Copy size={14} /></button
                    >
                    <button
                      class="small icon-only"
                      aria-label="Delete preset {p.name}"
                      title={p.in_use ? `Used by a logbook on this ${DEVICE}` : "Delete"}
                      disabled={p.in_use}
                      onclick={() => deletePreset(p)}><Trash2 size={14} /></button
                    >
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/each}
      {:else if section === "backups"}
        <h2>Backups</h2>
        <p class="muted">
          Independent, verified copies of the open logbook: automatically every interval, when the logbook
          closes, and on demand. Each backup is checked (hashes and database integrity) before it counts as
          successful. Old backups are removed only after a newer one succeeds.
        </p>
        <div class="field">
          <label for="st-dest">Backup destination</label>
          <div class="row">
            <input
              id="st-dest"
              type="text"
              readonly
              value={s.backup.destination ?? ""}
              placeholder="Not configured"
            /><button onclick={chooseBackup}
              ><FolderOpen size={16} />{IS_ANDROID ? "Choose…" : "Browse…"}</button
            >{#if s.backup.destination}<button onclick={() => (s.backup.destination = null)}>Turn off</button
              >{/if}
          </div>
          {#if backupCheck}<span class={backupCheck.ok ? "hint" : "field-error"}
              >{#if backupCheck.ok}<CircleCheck size={13} /> Usable{backupCheck.message
                  ? ` — ${backupCheck.message}`
                  : ""}{:else}<CircleAlert size={13} /> {backupCheck.message}{/if}</span
            >{/if}
          {#if !s.backup.destination}<span class="field-error"
              >No destination: no independent copies are being made.</span
            >{/if}
        </div>
        <div class="grid-4">
          <div class="field">
            <label for="st-int">Every (minutes)</label><input
              id="st-int"
              type="number"
              min="5"
              max="1440"
              bind:value={s.backup.interval_minutes}
            />
          </div>
          <div class="field">
            <label for="st-rec">Keep most recent</label><input
              id="st-rec"
              type="number"
              min="1"
              max="500"
              bind:value={s.backup.keep_recent}
            />
          </div>
          <div class="field">
            <label for="st-day">Daily copies</label><input
              id="st-day"
              type="number"
              min="0"
              max="365"
              bind:value={s.backup.keep_daily}
            />
          </div>
          <div class="field">
            <label for="st-wk">Weekly copies</label><input
              id="st-wk"
              type="number"
              min="0"
              max="520"
              bind:value={s.backup.keep_weekly}
            />
          </div>
        </div>
        <label class="check"
          ><input type="checkbox" bind:checked={s.backup.on_close} /> Back up when a logbook is closed</label
        >
        {#if app.active}
          <fieldset>
            <legend>Open logbook</legend>
            <div class="row wrap">
              <button onclick={backupNow} disabled={!app.backup?.configured || app.backup?.running || dirty}
                >Back Up Now</button
              >
              {#if dirty}<span class="hint">Save settings first</span>{/if}
              {#if app.backup?.last}<span class={app.backup.last.ok ? "badge ok" : "badge danger"}
                  >{app.backup.last.ok ? "LAST BACKUP SUCCEEDED" : "LAST BACKUP FAILED"}</span
                ><span class="muted"
                  >{app.backup.last.kind}, {relativeAge(app.backup.last.finished_at_utc)} — {app.backup.last
                    .message}</span
                >{/if}
            </div>
          </fieldset>
        {/if}
        {#if app.settings?.backup.destination}
          <div class="row">
            <h3>Backups at destination</h3>
            <span class="spacer"></span><button class="small" onclick={loadBackups}
              ><RefreshCw size={14} />Refresh</button
            ><button
              class="small"
              hidden={IS_ANDROID}
              onclick={() =>
                api
                  .revealPath(app.settings!.backup.destination!)
                  .catch((e) => app.toast("error", toAppError(e).message))}
              ><FolderSearch size={14} />Reveal</button
            ><button
              class="small"
              onclick={() => {
                app.dialog = "restore";
                app.navigate("home");
              }}>Restore…</button
            >
          </div>
          {#if backupsErr}<div class="field-error">{backupsErr}</div>{/if}
          {#if backups}
            <table class="data">
              <thead
                ><tr><th>Logbook</th><th>Created</th><th>Kind</th><th>Entries</th><th>Size</th></tr></thead
              >
              <tbody>
                {#each backups.slice(0, 50) as b (b.path)}
                  <tr
                    ><td>{b.name}</td><td class="nowrap"
                      >{formatInstant(b.created_at_utc, app.zone, app.clock)}</td
                    ><td>{b.kind}</td><td>{b.entry_count}</td><td>{Math.ceil(b.total_bytes / 1024)} KB</td
                    ></tr
                  >
                {:else}
                  <tr><td colspan="5" class="empty">No completed backups yet.</td></tr>
                {/each}
              </tbody>
            </table>
          {/if}
        {/if}
      {:else}
        <h2>Storage &amp; Recovery</h2>
        <dl class="kv">
          <dt>App data (settings, profiles, drafts)</dt>
          <dd>
            <span class="mono">{app.dataDir}</span>
            <button
              class="small"
              hidden={IS_ANDROID}
              onclick={() =>
                api.revealPath(app.dataDir).catch((e) => app.toast("error", toAppError(e).message))}
              ><FolderSearch size={14} />Reveal</button
            >
          </dd>
          <dt>Default logbook folder</dt>
          <dd class="mono">{app.settings?.default_logbook_parent ?? "—"}</dd>
          <dt>App version</dt>
          <dd>{app.appVersion} ({app.platform})</dd>
        </dl>
        {#if app.active}
          <fieldset>
            <legend>Open logbook</legend>
            <dl class="kv">
              <dt>Live folder</dt>
              <dd class="mono">{app.active.info.path}</dd>
              <dt>Logbook ID</dt>
              <dd class="mono">{app.active.info.id}</dd>
              <dt>Schema version</dt>
              <dd>{app.active.info.schema_version}</dd>
              <dt>Durability</dt>
              <dd>
                journal {app.active.health.journal_mode.toUpperCase()}, synchronous {app.active.health
                  .synchronous === 2
                  ? "FULL"
                  : app.active.health.synchronous}, foreign keys {app.active.health.foreign_keys
                  ? "on"
                  : "OFF"}
              </dd>
              <dt>Storage</dt>
              <dd>
                {#if app.storageBlocked}<span class="badge danger">UNAVAILABLE</span>
                  {app.storageBlocked.message}{:else}<span class="badge ok">OK</span>{/if}
                <button class="small" onclick={() => app.recheckStorage()}
                  ><RefreshCw size={14} />Recheck</button
                >
              </dd>
            </dl>
          </fieldset>
          <fieldset>
            <legend>Recoverable drafts</legend>
            {#if drafts.length === 0}<p class="muted" style="margin:0">
                No unfinished drafts for this logbook.
              </p>{/if}
            {#each drafts as d (d.draft_id)}
              <div class="row draft">
                <span class="grow"
                  ><strong>{d.editor === "edit" ? `Edit of entry #${d.entry_sequence}` : "New entry"}</strong>
                  — {(d.content as { form?: { subject?: string } }).form?.subject || "(no summary)"}
                  <span class="muted"
                    >· saved {formatInstant(d.saved_at_utc, app.zone, app.clock, { seconds: true })}</span
                  >{#if !d.intact}
                    <span class="badge danger">CHECKSUM MISMATCH</span>{/if}</span
                >
                <button class="small" onclick={() => restoreDraft(d)} disabled={!d.intact}>Restore</button>
                <button class="small danger" onclick={() => discardDraft(d)}>Discard</button>
              </div>
            {/each}
          </fieldset>
        {/if}
      {/if}

      {#if section === "location"}
        {@const g = s.gps!}
        <h2>Location &amp; GPS</h2>
        <p class="muted" style="margin:0">
          A position is taken only when you press <strong>Capture GPS</strong> on an entry or logbook, one fix at
          a time. EMCOMM LOG never tracks your position in the background and never uses the internet for location.
        </p>
        <div class="field">
          <label for="gps-pref">Preferred source</label>
          <select id="gps-pref" bind:value={g.preferred} style="max-width:420px">
            {#if IS_ANDROID}
              <option value="auto">Tablet location (GPS when the tablet has it)</option>
              <option value="manual">Manual only (never use location)</option>
            {:else}
              <option value="auto">Auto — a configured receiver first, then the computer's location</option>
              <option value="serial">External / built-in GPS on a serial port (USB or Bluetooth)</option>
              {#if app.platform === "linux"}<option value="gpsd">gpsd on this computer</option>{/if}
              <option value="os">Computer's location service (may be Wi-Fi based)</option>
              <option value="manual">Manual only (never use a receiver)</option>
            {/if}
          </select>
        </div>
        {#if IS_ANDROID}
          <p class="muted" style="margin:0">
            The tablet's own location is used: its satellite GPS when it has one, which works with no network,
            and otherwise Android's network location (labelled as an OS position). Android asks for permission
            the first time you press Capture or Test. External receivers are not used on the tablet.
          </p>
        {:else}
          <fieldset>
            <legend>Serial receiver (NMEA 0183)</legend>
            <div class="row wrap">
              <button class="small" onclick={detectReceivers}>Detect receivers</button>
              {#if gpsSources}<span class="hint"
                  >{gpsSources.filter((x) => x.provider === "serial").length} serial port(s) found</span
                >{/if}
            </div>
            {#if gpsSources?.some((x) => x.provider === "serial")}
              <ul class="ports">
                {#each gpsSources.filter((x) => x.provider === "serial") as src (src.path)}
                  <li>
                    <label class="check"
                      ><input
                        type="radio"
                        name="gps-port"
                        checked={g.serial_path === src.path}
                        onchange={() => {
                          g.serial_path = src.path;
                          if (src.serial_kind && src.serial_kind !== "unknown")
                            g.serial_kind = src.serial_kind;
                        }}
                      />
                      <span class="mono">{src.path}</span>{src.detail ? ` — ${src.detail}` : ""}</label
                    >
                  </li>
                {/each}
              </ul>
            {/if}
            <div class="grid-3">
              <div class="field">
                <label for="gps-path">Port</label>
                <input
                  id="gps-path"
                  type="text"
                  bind:value={g.serial_path}
                  placeholder={app.platform === "windows" ? "COM3" : "/dev/cu.usbserial-1410"}
                />
              </div>
              <div class="field">
                <label for="gps-baud">Baud rate</label>
                <select id="gps-baud" bind:value={g.baud}>
                  <option value={0}>Detect (4800, then 9600)</option>
                  {#each [4800, 9600, 19200, 38400, 57600, 115200] as b (b)}<option value={b}>{b}</option
                    >{/each}
                </select>
              </div>
              <div class="field">
                <label for="gps-kind">Receiver is</label>
                <select id="gps-kind" bind:value={g.serial_kind}>
                  <option value="usb">External, USB</option>
                  <option value="bluetooth">External, Bluetooth (serial)</option>
                  <option value="internal">Built into this computer</option>
                  <option value="unknown">Not sure</option>
                </select>
              </div>
            </div>
            <span class="hint"
              >Bluetooth receivers work when the system shows them as a serial port (pair them first).
              Receivers that only speak Bluetooth LE or a proprietary format are not supported. EMCOMM LOG
              never sends commands to the receiver.{app.platform === "linux"
                ? " On Linux your user needs to be in the 'dialout' group to open serial ports."
                : ""}</span
            >
          </fieldset>
        {/if}
        {#if app.platform === "linux"}
          <div class="field">
            <label for="gps-gpsd">gpsd address (this computer only)</label>
            <input id="gps-gpsd" type="text" bind:value={g.gpsd_endpoint} style="max-width:240px" />
          </div>
        {/if}
        <div class="row">
          <button onclick={() => (gpsTest = testRequest())} disabled={g.preferred === "manual"}
            >Test GPS</button
          >
          <span class="hint">Shows the source, fix, age and quality without saving anything.</span>
        </div>
      {/if}

      {#if section === "general" || section === "backups" || section === "location"}
        {#if saveErr}<div class="banner error" role="alert">
            <CircleAlert size={18} class="icon" />Settings not saved: {saveErr}
          </div>{/if}
        <div class="row">
          <span class="spacer"></span>
          <button onclick={revert} disabled={!dirty || saving}>Revert</button>
          <button class="primary" onclick={saveSettings} disabled={!dirty || saving}
            >{saving ? "Saving…" : "Save settings"}</button
          >
        </div>
      {/if}
    </div>
  </div>
</section>

{#if profileEdit}
  <Modal title={profileEdit.id ? "Edit profile" : "New profile"} onclose={() => (profileEdit = null)}>
    <div class="grid-2">
      <div class="field">
        <label for="pf-name">Name<span class="req">*</span></label><input
          id="pf-name"
          type="text"
          bind:value={profileEdit.input.name}
          data-autofocus
        />
      </div>
      <div class="field">
        <label for="pf-call">Callsign</label><input
          id="pf-call"
          type="text"
          bind:value={profileEdit.input.callsign}
        />
      </div>
      <div class="field">
        <label for="pf-lic">License class</label><input
          id="pf-lic"
          type="text"
          list="pf-lic-list"
          bind:value={profileEdit.input.license_class}
        />
        <datalist id="pf-lic-list"
          >{#each ["Unlicensed", "Technician", "General", "Amateur Extra", "Advanced (legacy)", "Novice (legacy)", "Other"] as c (c)}<option
              value={c}
            ></option>{/each}</datalist
        >
      </div>
      <div class="field">
        <label for="pf-country">Licensing country</label><input
          id="pf-country"
          type="text"
          bind:value={profileEdit.input.license_country}
          placeholder="e.g. United States"
        />
      </div>
      <div class="field">
        <label for="pf-unit">Unit / team</label><input
          id="pf-unit"
          type="text"
          bind:value={profileEdit.input.unit}
        />
      </div>
      <div class="field">
        <label for="pf-grid">Grid</label><input
          id="pf-grid"
          type="text"
          bind:value={profileEdit.input.grid}
        />
      </div>
      <div class="field">
        <label for="pf-org">Organization</label><input
          id="pf-org"
          type="text"
          bind:value={profileEdit.input.organization}
        />
      </div>
      <div class="field">
        <label for="pf-pos">Position</label><input
          id="pf-pos"
          type="text"
          bind:value={profileEdit.input.position}
        />
      </div>
    </div>
    {#if profileErr}<div class="banner error" role="alert">{profileErr}</div>{/if}
    {#snippet footer()}<button onclick={() => (profileEdit = null)}>Cancel</button><button
        class="primary"
        onclick={saveProfile}>Save profile</button
      >{/snippet}
  </Modal>
{/if}

{#if gpsTest}<GpsCaptureDialog request={gpsTest} onclose={() => (gpsTest = null)} />{/if}

{#if presetEdit}
  {@const pt = presetEdit.input.logbook_type!}
  <Modal
    title={presetEdit.id ? "Edit preset" : `New ${TYPE_LABEL[pt]} preset`}
    size="lg"
    onclose={() => (presetEdit = null)}
  >
    <div class="grid-2">
      <div class="field">
        <label for="ps-name">Name<span class="req">*</span></label><input
          id="ps-name"
          type="text"
          bind:value={presetEdit.input.name}
          data-autofocus
        />
      </div>
      <div class="field">
        <label for="ps-prof">Linked profile</label>
        <select id="ps-prof" bind:value={presetEdit.input.profile_id}
          ><option value={null}>Any profile</option>{#each app.profiles as p (p.id)}<option value={p.id}
              >{p.name}</option
            >{/each}</select
        >
      </div>
      <div class="field">
        <label for="ps-kind">Default entry type</label>
        <select id="ps-kind" bind:value={presetEdit.input.config.default_kind}
          ><option value={null}>—</option>{#each allowedKinds(pt) as k (k)}<option value={k}
              >{KIND_LABEL[k]}</option
            >{/each}</select
        >
      </div>
      <div class="field">
        <label for="ps-dir">Default direction</label>
        <select id="ps-dir" bind:value={presetEdit.input.config.default_direction}
          ><option value={null}>—</option>{#each Object.entries(DIRECTION_LABEL) as [v, l] (v)}<option
              value={v}>{l}</option
            >{/each}</select
        >
      </div>
      <div class="field">
        <label for="ps-pri">Default priority</label>
        <select id="ps-pri" bind:value={presetEdit.input.config.default_priority}
          ><option value={null}>—</option>{#each Object.entries(PRIORITY_LABEL) as [v, l] (v)}<option
              value={v}>{l}</option
            >{/each}</select
        >
      </div>
      <div class="field">
        <label for="ps-ch">Default channel</label><input
          id="ps-ch"
          type="text"
          bind:value={presetEdit.input.config.default_channel}
        />
      </div>
      {#if pt === "ham"}<div class="field">
          <label for="ps-mode">Default mode</label><input
            id="ps-mode"
            type="text"
            bind:value={presetEdit.input.config.default_mode}
          />
        </div>{/if}
    </div>
    <TagInput id="ps-tags" label="Quick tags" bind:tags={presetEdit.input.config.quick_tags} />
    <TagInput
      id="ps-channels"
      label="Channel list (suggestions)"
      bind:tags={presetEdit.input.config.channels}
    />
    <fieldset>
      <legend>Hide optional fields</legend>
      <div class="row wrap">
        {#each HIDEABLE as f (f)}<label class="check"
            ><input
              type="checkbox"
              checked={presetEdit.input.config.hidden_fields.includes(f)}
              onchange={() => toggleHidden(f)}
            />{f.replace("_text", "").replace("_", " ")}</label
          >{/each}
      </div>
      <span class="hint">Time, type, direction, priority and summary are always shown.</span>
    </fieldset>
    <fieldset>
      <legend>Custom field labels</legend>
      <div class="grid-3">
        {#each LABELABLE as f (f)}<div class="field">
            <label for="ps-l-{f}">{f.replace("_text", "")}</label><input
              id="ps-l-{f}"
              type="text"
              maxlength="40"
              value={presetEdit.input.config.field_labels[f] ?? ""}
              oninput={(e) => setLabel(f, e.currentTarget.value)}
            />
          </div>{/each}
      </div>
    </fieldset>
    <fieldset>
      <legend>Starter templates ({presetEdit.input.config.templates.length})</legend>
      {#each presetEdit.input.config.templates as t, i (i)}
        <div class="row">
          <span class="grow">{t.name} <span class="muted">— {t.defaults.subject}</span></span><button
            class="small icon-only"
            aria-label="Remove template {t.name}"
            onclick={() =>
              presetEdit &&
              (presetEdit.input.config.templates = presetEdit.input.config.templates.filter(
                (_, j) => j !== i,
              ))}><Trash2 size={14} /></button
          >
        </div>
      {/each}
      {#if app.active?.info.logbook_type === pt}<button class="small" onclick={copyTemplatesFromLogbook}
          >Replace with templates from the open logbook</button
        >{/if}
    </fieldset>
    {#if presetErr}<div class="banner error" role="alert">{presetErr}</div>{/if}
    {#snippet footer()}<button onclick={() => (presetEdit = null)}>Cancel</button><button
        class="primary"
        onclick={savePreset}>Save preset</button
      >{/snippet}
  </Modal>
{/if}

<style>
  .title {
    display: flex;
    gap: 10px;
    align-items: center;
    font-size: 20px;
  }
  .layout {
    display: grid;
    grid-template-columns: 210px 1fr;
    min-height: 480px;
  }
  .secnav {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 14px 10px;
    border-right: 1px solid var(--border);
  }
  .secnav button {
    border: none;
    background: transparent;
    justify-content: flex-start;
  }
  .secnav button[aria-current="page"] {
    background: var(--panel-3);
    color: var(--cyan);
    font-weight: 600;
  }
  .content {
    padding: 16px 20px;
    gap: 14px;
    min-width: 0;
  }
  .content p {
    margin: 0;
  }
  .kv {
    display: grid;
    grid-template-columns: 240px 1fr;
    gap: 8px 14px;
    margin: 0;
  }
  .kv dt {
    color: var(--muted);
  }
  .kv dd {
    margin: 0;
    word-break: break-all;
  }
  .draft {
    padding: 6px 0;
    border-bottom: 1px solid var(--border);
  }
</style>
