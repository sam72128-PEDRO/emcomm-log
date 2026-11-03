<script lang="ts">
  import { FolderOpen, CircleAlert, CircleCheck, Radio, NotebookPen, Siren } from "@lucide/svelte";
  import Modal from "./Modal.svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import { closeActiveLogbook } from "../lib/actions";
  import type { FolderCheck, IncidentFields, LogbookType } from "../lib/types";

  let { onclose, oncreated }: { onclose: () => void; oncreated: () => void } = $props();

  let type = $state<LogbookType>("general");
  let name = $state("");
  let profileId = $state(app.settings?.active_profile_id ?? app.profiles.find((p) => p.active)?.id ?? "");
  let presetId = $state<string | null>(null);
  let parent = $state(app.settings?.default_logbook_parent ?? "");
  let parentCheck = $state<FolderCheck | null>(null);
  let incident = $state<IncidentFields>({
    incident_name: "",
    incident_number: "",
    operational_period: "",
    agency: "",
    location: "",
  });
  let errors = $state<Record<string, string>>({});
  let busy = $state(false);
  let failure = $state<string | null>(null);

  const presetsForType = $derived(
    app.presets.filter((p) => p.logbook_type === type && (!p.profile_id || p.profile_id === profileId)),
  );
  $effect(() => {
    const def = app.settings?.default_preset_ids[type];
    presetId = presetsForType.find((p) => p.id === def)?.id ?? presetsForType[0]?.id ?? null;
  });
  $effect(() => {
    if (parent)
      api
        .checkFolder(parent, "live")
        .then((c) => (parentCheck = c))
        .catch(() => {});
  });

  const typeCards: { t: LogbookType; label: string; desc: string; icon: typeof Radio }[] = [
    { t: "general", label: "General", desc: "Routine communications and notes", icon: NotebookPen },
    { t: "ham", label: "HAM", desc: "Amateur radio QSOs and traffic, ADIF export", icon: Radio },
    { t: "emergency", label: "Emergency", desc: "Activation messages and activity records", icon: Siren },
  ];

  async function browse() {
    const p = await api.pickFolder("Choose where to create the logbook folder", parent || null);
    if (p) parent = p;
  }

  async function create() {
    errors = {};
    failure = null;
    if (!name.trim()) errors.name = "Name is required";
    if (!profileId) errors.profile = "Choose an operator profile";
    if (!parent) errors.parent = "Choose a storage folder";
    else if (parentCheck && !parentCheck.ok) errors.parent = parentCheck.message ?? "Folder not usable";
    if (type === "emergency" && !incident.incident_name.trim())
      errors.incident_name = "Incident name is required";
    const first = Object.keys(errors)[0];
    if (first) {
      app.announce(errors[first], true);
      document.getElementById(`cl-${first}`)?.focus();
      return;
    }
    if (app.active && !(await closeActiveLogbook())) return;
    busy = true;
    try {
      const view = await api.createLogbook({
        logbook_type: type,
        name: name.trim(),
        parent_dir: parent,
        profile_id: profileId,
        preset_id: presetId,
        incident,
      });
      app.setActive(view);
      app.saveStatus = { kind: "idle" };
      app.toast("ok", `Created logbook "${view.info.name}"`);
      oncreated();
      onclose();
      app.screen = "dashboard";
    } catch (e) {
      const err = toAppError(e);
      if (err.field === "name" || err.code === "ALREADY_EXISTS") errors.name = err.message;
      failure = err.message;
      app.announce(err.message, true);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title="Create logbook" size="lg" {onclose} {busy}>
  <fieldset>
    <legend>Logbook type<span class="req" aria-hidden="true">*</span></legend>
    <div class="types" role="radiogroup" aria-label="Logbook type">
      {#each typeCards as c (c.t)}
        <label class="type" class:sel={type === c.t}>
          <input type="radio" name="lbtype" value={c.t} bind:group={type} class="sr-only" />
          <c.icon size={26} aria-hidden="true" />
          <strong>{c.label}</strong>
          <span class="muted">{c.desc}</span>
        </label>
      {/each}
    </div>
  </fieldset>
  <div class="grid-2">
    <div class="field">
      <label for="cl-name">Name<span class="req">*</span></label>
      <input
        id="cl-name"
        type="text"
        bind:value={name}
        maxlength="120"
        data-autofocus
        aria-invalid={errors.name ? "true" : undefined}
      />
      {#if errors.name}<span class="field-error">{errors.name}</span>{/if}
    </div>
    <div class="field">
      <label for="cl-profile">Operator profile<span class="req">*</span></label>
      <select id="cl-profile" bind:value={profileId} aria-invalid={errors.profile ? "true" : undefined}>
        {#each app.profiles.filter((p) => p.active) as p (p.id)}<option value={p.id}
            >{p.name}{p.callsign ? ` (${p.callsign})` : ""}</option
          >{/each}
      </select>
      {#if errors.profile}<span class="field-error">{errors.profile}</span>{/if}
    </div>
    <div class="field">
      <label for="cl-preset">Preset</label>
      <select id="cl-preset" bind:value={presetId}>
        {#each presetsForType as p (p.id)}<option value={p.id}>{p.name} (v{p.version})</option>{/each}
        <option value={null}>No preset</option>
      </select>
      <span class="hint">Copied into the logbook now; later preset edits won't change it.</span>
    </div>
    <div class="field">
      <label for="cl-parent">Storage folder<span class="req">*</span></label>
      <div class="row">
        <input
          id="cl-parent"
          type="text"
          readonly
          value={parent}
          aria-invalid={errors.parent ? "true" : undefined}
        />
        <button onclick={browse}><FolderOpen size={16} />Browse…</button>
      </div>
      {#if errors.parent}<span class="field-error">{errors.parent}</span>{:else if parentCheck}<span
          class={parentCheck.ok ? "hint" : "field-error"}
          >{#if parentCheck.ok}<CircleCheck size={13} /> Writable local folder{:else}<CircleAlert size={13} />
            {parentCheck.message}{/if}</span
        >{/if}
    </div>
  </div>
  {#if type === "emergency"}
    <fieldset>
      <legend>Incident</legend>
      <div class="grid-2">
        <div class="field">
          <label for="cl-incident_name">Incident name<span class="req">*</span></label>
          <input
            id="cl-incident_name"
            type="text"
            bind:value={incident.incident_name}
            aria-invalid={errors.incident_name ? "true" : undefined}
          />
          {#if errors.incident_name}<span class="field-error">{errors.incident_name}</span>{/if}
        </div>
        <div class="field">
          <label for="cl-incident-number">Incident number (optional)</label>
          <input id="cl-incident-number" type="text" bind:value={incident.incident_number} />
        </div>
        <div class="field">
          <label for="cl-op">Operational period</label>
          <input
            id="cl-op"
            type="text"
            bind:value={incident.operational_period}
            placeholder="e.g. OP03 1200–2000"
          />
        </div>
        <div class="field">
          <label for="cl-agency">Agency / organization</label>
          <input id="cl-agency" type="text" bind:value={incident.agency} />
        </div>
        <div class="field" style="grid-column: span 2">
          <label for="cl-loc">Location</label>
          <input id="cl-loc" type="text" bind:value={incident.location} />
        </div>
      </div>
    </fieldset>
  {/if}
  {#if failure}<div class="banner error" role="alert">
      <CircleAlert size={18} class="icon" />{failure}
    </div>{/if}
  {#snippet footer()}
    <button onclick={onclose} disabled={busy}>Cancel</button>
    <button class="primary" onclick={create} disabled={busy}>{busy ? "Creating…" : "Create and open"}</button>
  {/snippet}
</Modal>

<style>
  .types {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 10px;
  }
  .type {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 12px;
    border: 1.5px solid var(--border-strong);
    border-radius: 9px;
    cursor: pointer;
    color: var(--text);
  }
  .type :global(svg) {
    color: var(--cyan);
  }
  .type.sel {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }
  .type:focus-within {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }
</style>
