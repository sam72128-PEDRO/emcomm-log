<script lang="ts">
  import { onMount, tick } from "svelte";
  import type { Snippet } from "svelte";
  import { X } from "@lucide/svelte";
  import { app } from "../lib/state.svelte";

  let {
    title,
    onclose,
    children,
    footer,
    size = "md",
    busy = false,
  }: {
    title: string;
    onclose: () => void;
    children: Snippet;
    footer?: Snippet;
    size?: "sm" | "md" | "lg" | "xl";
    busy?: boolean;
  } = $props();

  let dialogEl: HTMLDivElement | undefined = $state();
  const titleId = `dlg-${Math.random().toString(36).slice(2)}`;
  let returnFocus: HTMLElement | null = null;

  const FOCUSABLE =
    'a[href], button:not([disabled]), input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

  onMount(() => {
    returnFocus = document.activeElement as HTMLElement | null;
    app.modalDepth++;
    tick().then(() => {
      const auto = dialogEl?.querySelector<HTMLElement>("[data-autofocus]");
      const first = auto ?? dialogEl?.querySelector<HTMLElement>(`.modal-body ${FOCUSABLE}`) ?? dialogEl;
      first?.focus();
    });
    return () => {
      app.modalDepth = Math.max(0, app.modalDepth - 1);
      if (returnFocus && document.contains(returnFocus)) returnFocus.focus();
    };
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.stopPropagation();
      e.preventDefault();
      if (!busy) onclose();
      return;
    }
    if (e.key !== "Tab" || !dialogEl) return;
    const items = [...dialogEl.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
      (el) => el.offsetParent !== null,
    );
    if (items.length === 0) return;
    const first = items[0];
    const last = items[items.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }
</script>

<div class="backdrop">
  <div
    class="modal {size}"
    role="dialog"
    aria-modal="true"
    aria-labelledby={titleId}
    aria-busy={busy}
    tabindex="-1"
    bind:this={dialogEl}
    {onkeydown}
  >
    <header class="modal-head">
      <h2 id={titleId}>{title}</h2>
      <span class="spacer"></span>
      <button class="ghost icon-only" aria-label="Close dialog" onclick={onclose} disabled={busy}
        ><X size={18} /></button
      >
    </header>
    <div class="modal-body">
      {@render children()}
    </div>
    {#if footer}
      <footer class="modal-foot">{@render footer()}</footer>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(2, 8, 16, 0.62);
    display: grid;
    place-items: center;
    z-index: 50;
    padding: 24px;
  }
  .modal {
    background: var(--panel);
    border: 1px solid var(--border-strong);
    border-radius: 12px;
    box-shadow: var(--shadow);
    display: flex;
    flex-direction: column;
    max-height: calc(100vh - 48px);
    width: 520px;
    max-width: 100%;
  }
  .modal.sm {
    width: 420px;
  }
  .modal.lg {
    width: 760px;
  }
  .modal.xl {
    width: 1000px;
  }
  .modal-head {
    display: flex;
    align-items: center;
    padding: 14px 18px;
    border-bottom: 1px solid var(--border);
  }
  .modal-body {
    padding: 16px 18px;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .modal-foot {
    display: flex;
    gap: 10px;
    justify-content: flex-end;
    align-items: center;
    padding: 12px 18px;
    border-top: 1px solid var(--border);
    flex-wrap: wrap;
  }
</style>
