<script lang="ts">
  import { CircleCheck, ExternalLink, FolderSearch } from "@lucide/svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import type * as T from "../lib/types";

  let { result, noun }: { result: T.FormResult; noun: string } = $props();
  const n = (count: number, one: string, many: string) => `${count} ${count === 1 ? one : many}`;
</script>

<div class="banner ok">
  <CircleCheck size={18} class="icon" />
  <div class="grow">
    <strong>{noun}: {n(result.exported, "entry", "entries")}, {n(result.pages, "page", "pages")}</strong>
    saved to <span class="mono">{result.path}</span> ({Math.ceil(result.bytes / 1024)} KB).
    {#if result.skipped.length}
      <details>
        <summary>{n(result.skipped.length, "selected entry", "selected entries")} not included</summary>
        <ul>
          {#each result.skipped as sk (sk.sequence_no)}<li>#{sk.sequence_no}: {sk.reason}</li>{/each}
        </ul>
      </details>
    {/if}
    {#if result.warnings.length}
      <details>
        <summary>{result.warnings.length} note{result.warnings.length === 1 ? "" : "s"}</summary>
        <ul>
          {#each result.warnings as w, i (i)}<li>{w}</li>{/each}
        </ul>
      </details>
    {/if}
  </div>
  <button
    class="small"
    onclick={() => api.openExport(result.path).catch((e) => app.toast("error", toAppError(e).message))}
    ><ExternalLink size={14} />Open</button
  >
  <button
    class="small"
    onclick={() => api.revealPath(result.path).catch((e) => app.toast("error", toAppError(e).message))}
    ><FolderSearch size={14} />Reveal</button
  >
</div>
