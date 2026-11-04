<script lang="ts">
  import { Plus, Pencil, Trash2, Search } from "@lucide/svelte";
  import DirectoryDialog from "./DirectoryDialog.svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import { confirm } from "../lib/confirm.svelte";
  import { KIND_LABEL } from "../lib/directory";
  import { formatInstant } from "../lib/time";
  import type * as T from "../lib/types";

  let query = $state("");
  let items = $state<T.DirectoryEntry[]>([]);
  let loading = $state(true);
  let editing = $state<T.DirectoryEntry | null>(null);
  let adding = $state(false);

  async function load() {
    loading = true;
    try {
      items = await api.listDirectory(query);
    } catch (e) {
      app.toast("error", toAppError(e).message);
    } finally {
      loading = false;
    }
  }
  $effect(() => {
    void query;
    void app.dataVersion;
    void load();
  });

  async function remove(d: T.DirectoryEntry) {
    const a = await confirm.ask({
      title: "Remove from directory?",
      message: `Remove "${d.label}" from this logbook's directory?`,
      detail: "Entries that used it keep their own text; nothing in the Traffic Log changes.",
      buttons: [
        { id: "cancel", label: "Cancel" },
        { id: "remove", label: "Remove", tone: "danger" },
      ],
      cancelId: "cancel",
    });
    if (a.id !== "remove") return;
    try {
      await api.deleteDirectory(d.id);
      app.toast("ok", `${d.label} removed from the directory.`);
      await load();
    } catch (e) {
      app.toast("error", toAppError(e).message);
    }
  }
</script>

<h2>Logbook directory</h2>
<p class="muted" style="margin:0">
  Stations, tactical identifiers and organizations you work with in <strong>{app.active?.info.name}</strong>.
  They are offered first while you type in an entry, are kept only in this logbook (and its .emlog packages),
  and never change entries already saved.
</p>
<div class="row wrap">
  <div class="search">
    <Search size={16} aria-hidden="true" />
    <input
      type="search"
      placeholder="Search name, identifier, alias, tag"
      bind:value={query}
      aria-label="Search directory"
    />
  </div>
  <span class="spacer"></span>
  <button class="primary" onclick={() => (adding = true)} disabled={app.readOnly}
    ><Plus size={16} />Add</button
  >
</div>
{#if loading && !items.length}<p class="muted">Loading…</p>
{:else if !items.length}<p class="muted">
    {query
      ? "Nothing matches."
      : "The directory is empty. Add stations here, or use Save to Directory on an entry."}
  </p>
{:else}
  <table class="table">
    <thead
      ><tr
        ><th>Name</th><th>Type</th><th>Identifier</th><th>Agency</th><th>Channel</th><th>Last used</th><th
        ></th></tr
      ></thead
    >
    <tbody>
      {#each items as d (d.id)}
        <tr>
          <td
            >{d.label}{#if d.aliases.length}<div class="muted small">aka {d.aliases.join(", ")}</div>{/if}</td
          >
          <td>{KIND_LABEL[d.kind]}</td>
          <td class="mono">{d.identifier}</td>
          <td>{d.agency}</td>
          <td
            >{d.channel}{d.frequency_hz ? ` · ${(d.frequency_hz / 1e6).toFixed(4)} MHz` : ""}{d.mode
              ? ` · ${d.mode}`
              : ""}</td
          >
          <td class="muted">{d.last_used_utc ? formatInstant(d.last_used_utc, app.zone, app.clock) : "—"}</td>
          <td class="actions">
            <button
              class="small ghost"
              onclick={() => (editing = d)}
              disabled={app.readOnly}
              aria-label="Edit {d.label}"><Pencil size={14} /></button
            >
            <button
              class="small ghost"
              onclick={() => remove(d)}
              disabled={app.readOnly}
              aria-label="Remove {d.label}"><Trash2 size={14} /></button
            >
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
{/if}

{#if adding}<DirectoryDialog onclose={() => (adding = false)} onsaved={() => load()} />{/if}
{#if editing}<DirectoryDialog item={editing} onclose={() => (editing = null)} onsaved={() => load()} />{/if}

<style>
  .search {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 1;
    max-width: 420px;
  }
  .search input {
    flex: 1;
  }
  .actions {
    white-space: nowrap;
    text-align: right;
  }
  .small {
    font-size: 12.5px;
  }
</style>
