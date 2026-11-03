<script lang="ts">
  import { CircleCheck, CircleAlert, TriangleAlert, Info, X } from "@lucide/svelte";
  import { app } from "../lib/state.svelte";
</script>

<!-- Screen-reader announcements for saves, failures, and shortcut feedback. -->
<div class="sr-only" role="status" aria-live="polite" aria-atomic="true">{app.politeMessage}</div>
<div class="sr-only" role="alert" aria-live="assertive" aria-atomic="true">{app.assertiveMessage}</div>

<div class="toasts" aria-hidden="true">
  {#each app.toasts as t (t.id)}
    <div class="toast {t.tone}">
      {#if t.tone === "ok"}<CircleCheck size={18} />{:else if t.tone === "error"}<CircleAlert
          size={18}
        />{:else if t.tone === "warn"}<TriangleAlert size={18} />{:else}<Info size={18} />{/if}
      <span class="grow">{t.text}</span>
      <button
        class="ghost icon-only small"
        tabindex="-1"
        aria-label="Dismiss"
        onclick={() => app.dismissToast(t.id)}><X size={14} /></button
      >
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: 18px;
    bottom: 18px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    z-index: 60;
    width: 380px;
    max-width: calc(100vw - 36px);
  }
  .toast {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 10px 12px;
    border-radius: 9px;
    background: var(--panel);
    border: 1px solid var(--border-strong);
    box-shadow: var(--shadow);
  }
  .toast.ok {
    border-color: var(--ok);
    color: var(--text);
  }
  .toast.ok :global(svg:first-child) {
    color: var(--ok);
  }
  .toast.error {
    border-color: var(--danger);
  }
  .toast.error :global(svg:first-child) {
    color: var(--danger);
  }
  .toast.warn {
    border-color: var(--warn);
  }
  .toast.warn :global(svg:first-child) {
    color: var(--warn);
  }
</style>
