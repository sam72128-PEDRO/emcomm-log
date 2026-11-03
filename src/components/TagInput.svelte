<script lang="ts">
  import { X } from "@lucide/svelte";

  let {
    tags = $bindable(),
    quick = [],
    id,
    label = "Tags",
    onchange,
    disabled = false,
  }: {
    tags: string[];
    quick?: string[];
    id: string;
    label?: string;
    onchange?: () => void;
    disabled?: boolean;
  } = $props();

  let text = $state("");

  function add(t: string) {
    const v = t.trim().replace(/,/g, " ").slice(0, 40);
    if (!v || tags.some((x) => x.toLowerCase() === v.toLowerCase()) || tags.length >= 20) return;
    tags = [...tags, v];
    onchange?.();
  }
  function remove(t: string) {
    tags = tags.filter((x) => x !== t);
    onchange?.();
  }
  function onkeydown(e: KeyboardEvent) {
    if ((e.key === "Enter" || e.key === ",") && text.trim()) {
      e.preventDefault();
      add(text);
      text = "";
    } else if (e.key === "Backspace" && !text && tags.length) {
      remove(tags[tags.length - 1]);
    }
  }
</script>

<div class="field">
  <label for={id}>{label}</label>
  <div class="tagbox">
    {#each tags as t (t)}
      <span class="badge neutral tag"
        >{t}<button
          type="button"
          class="ghost icon-only small"
          aria-label="Remove tag {t}"
          onclick={() => remove(t)}
          {disabled}><X size={12} /></button
        ></span
      >
    {/each}
    <input
      {id}
      type="text"
      bind:value={text}
      {onkeydown}
      onblur={() => {
        if (text.trim()) {
          add(text);
          text = "";
        }
      }}
      placeholder={tags.length ? "" : "Type and press Enter"}
      aria-describedby="{id}-hint"
      {disabled}
    />
  </div>
  {#if quick.length}
    <div class="row wrap" style="gap:6px" id="{id}-hint">
      <span class="hint">Quick:</span>
      {#each quick as q (q)}
        <button
          type="button"
          class="small"
          aria-pressed={tags.includes(q)}
          onclick={() => (tags.includes(q) ? remove(q) : add(q))}
          {disabled}>{q}</button
        >
      {/each}
    </div>
  {:else}
    <span class="hint" id="{id}-hint">Press Enter or comma to add a tag.</span>
  {/if}
</div>

<style>
  .tagbox {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
    align-items: center;
    border: 1px solid var(--border-strong);
    background: var(--input-bg);
    border-radius: 7px;
    padding: 3px 6px;
    min-height: 34px;
  }
  .tagbox input {
    border: none !important;
    outline: none !important;
    flex: 1;
    min-width: 100px;
    padding: 4px;
    min-height: 26px;
    background: transparent;
  }
  .tag :global(button) {
    min-height: 18px;
    min-width: 18px;
    padding: 0;
    color: inherit;
  }
  button[aria-pressed="true"] {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 22%, transparent);
  }
</style>
