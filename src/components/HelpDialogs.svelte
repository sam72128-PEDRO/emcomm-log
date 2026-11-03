<script lang="ts">
  import { IS_ANDROID, DEVICE } from "../lib/platform";
  import Modal from "./Modal.svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import { SHORTCUTS, modKey } from "../lib/actions";

  const close = () => (app.dialog = null);
</script>

{#if app.dialog === "shortcuts"}
  <Modal title="Keyboard shortcuts" onclose={close}>
    <table class="data">
      <tbody>
        {#each SHORTCUTS as s (s.action)}
          <tr><td><kbd>{modKey()}{s.keys}</kbd></td><td>{s.label}</td></tr>
        {/each}
        <tr><td><kbd>Esc</kbd></td><td>Close dialog / cancel the current non-saving action</td></tr>
        <tr><td><kbd>Tab</kbd> / <kbd>Shift+Tab</kbd></td><td>Move between fields in order</td></tr>
        <tr
          ><td><kbd>↑</kbd> <kbd>↓</kbd> <kbd>Enter</kbd> <kbd>Space</kbd></td><td
            >Traffic Log: move between rows, open details, select a row</td
          ></tr
        >
      </tbody>
    </table>
    <p class="hint" style="margin:0">Shortcuts other than Save are disabled while a dialog is open.</p>
  </Modal>
{:else if app.dialog === "data"}
  <Modal title="Where is my data?" size="lg" onclose={close}>
    <p style="margin:0">
      <strong>Logbooks</strong> are {#if IS_ANDROID}folders in EMCOMM LOG's private storage on this tablet{:else}folders
        you choose{/if}. Each contains <span class="mono">logbook.sqlite</span>
      (every entry, revision, template and setting), an <span class="mono">attachments/</span> folder, and a
      small <span class="mono">logbook.json</span> identity file. Save Entry and Save Changes write directly to
      that database — there is no separate "save logbook" step.
    </p>
    {#if app.active}<p style="margin:0">
        Open logbook: <span class="mono">{app.active.info.path}</span>
      </p>{/if}
    <p style="margin:0">
      <strong>Drafts</strong> of unfinished entries, operator profiles, presets and app settings are stored on
      this {DEVICE} only, in <span class="mono">{app.dataDir}</span>.
    </p>
    <p style="margin:0">
      <strong>Backups</strong> are verified copies in the backup folder set in Settings → Backups{#if app.settings?.backup.destination}:
        <span class="mono">{app.settings.backup.destination}</span>{/if}.
    </p>
    <h3>If the logbook location becomes unavailable</h3>
    <ol style="margin:0">
      <li>
        Final saves are blocked so nothing is written to the wrong place. What you typed stays in the form and
        in a local recovery draft.
      </li>
      <li>Reconnect the drive (or restore folder access) and choose <em>Recheck storage</em>.</li>
      <li>
        If the drive cannot be recovered, close the logbook and restore the latest backup as a new logbook
        from Logbooks → Restore Backup. Your drafts can then be re-entered.
      </li>
    </ol>
    {#snippet footer()}
      {#if !IS_ANDROID}<button
          onclick={() => api.revealPath(app.dataDir).catch((e) => app.toast("error", toAppError(e).message))}
          >Reveal app data folder</button
        >{/if}
      {#if IS_ANDROID}<button onclick={() => (app.dialog = "shortcuts")}>Keyboard shortcuts</button><button
          onclick={() => (app.dialog = "about")}>About</button
        >{/if}
      <button class="primary" onclick={close}>Close</button>
    {/snippet}
  </Modal>
{:else if app.dialog === "about"}
  <Modal title="About EMCOMM LOG" size="sm" onclose={close}>
    <p style="margin:0"><strong>EMCOMM LOG</strong> version {app.appVersion}</p>
    <p class="muted" style="margin:0">
      Offline, single-user communications logbook for general, amateur radio, and emergency operations. No
      network connection, accounts, or telemetry.
    </p>
    <p class="muted" style="margin:0">
      PDF reports use the DejaVu Sans font (Bitstream Vera / DejaVu license).
    </p>
  </Modal>
{/if}
