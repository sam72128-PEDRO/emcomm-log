<script lang="ts">
  import Modal from "./Modal.svelte";
  import { confirm } from "../lib/confirm.svelte";

  let value = $derived(confirm.current?.input?.value ?? "");
</script>

{#if confirm.current}
  {@const c = confirm.current}
  <Modal title={c.title} size="sm" onclose={() => confirm.answer(c.cancelId, value)}>
    <p style="margin:0">{c.message}</p>
    {#if c.detail}<p class="muted" style="margin:0">{c.detail}</p>{/if}
    {#if c.input}
      <div class="field">
        <label for="confirm-input">{c.input.label}</label>
        <input
          id="confirm-input"
          type="text"
          bind:value
          maxlength={c.input.maxLength ?? 500}
          data-autofocus
        />
      </div>
    {/if}
    {#snippet footer()}
      {#each c.buttons as b, i (b.id)}
        <button
          class={b.tone === "primary" ? "primary" : b.tone === "danger" ? "danger" : ""}
          data-autofocus={!c.input && i === 0 ? true : undefined}
          onclick={() => confirm.answer(b.id, value)}>{b.label}</button
        >
      {/each}
    {/snippet}
  </Modal>
{/if}
