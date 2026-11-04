<script lang="ts">
  // 2.2: saved attachments of an entry. Thumbnails are small re-encoded copies (no EXIF). Files open only
  // when the operator asks; programs and scripts are never opened (Save As instead).
  import { FileText, ExternalLink, Download } from "@lucide/svelte";
  import { app } from "../lib/state.svelte";
  import { api, toAppError } from "../lib/api";
  import type * as T from "../lib/types";

  let { attachments }: { attachments: T.Attachment[] } = $props();
  let thumbs = $state<Record<string, string | null>>({});
  const kb = (n: number) =>
    n >= 1024 * 1024 ? `${(n / 1024 / 1024).toFixed(1)} MB` : `${Math.ceil(n / 1024)} KB`;
  const SOURCE: Record<string, string> = {
    drag_drop: "dropped",
    paste: "pasted",
    camera: "camera",
    photo_picker: "photo library",
    document_picker: "documents",
  };

  $effect(() => {
    for (const a of attachments) {
      if (a.id in thumbs || !/^image\/(jpeg|png|gif|webp)$/.test(a.mime)) continue;
      thumbs[a.id] = null;
      api
        .attachmentThumbnail(a.id, null)
        .then((t) => (thumbs[a.id] = t))
        .catch(() => {});
    }
  });

  async function saveAs(a: T.Attachment) {
    try {
      const p = await api.saveAttachmentAs(a.id);
      if (p) app.toast("ok", `Saved a copy of ${a.display_name}.`);
    } catch (e) {
      app.toast("error", toAppError(e).message);
    }
  }
</script>

<ul class="gallery">
  {#each attachments as a (a.id)}
    <li>
      {#if thumbs[a.id]}<img src={thumbs[a.id]} alt={a.caption || a.display_name} />{:else}<span class="icon"
          ><FileText size={26} /></span
        >{/if}
      <div class="meta">
        <strong title={a.display_name}>{a.display_name}</strong>
        {#if a.caption}<span>{a.caption}</span>{/if}
        <span class="muted small"
          >{kb(a.size_bytes)}{a.source && SOURCE[a.source] ? ` · ${SOURCE[a.source]}` : ""} · SHA-256 {a.sha256.slice(
            0,
            12,
          )}…</span
        >
        <div class="row">
          <button
            class="small"
            onclick={() => api.openAttachment(a.id).catch((e) => app.toast("error", toAppError(e).message))}
            ><ExternalLink size={13} />Open</button
          >
          <button class="small" onclick={() => saveAs(a)}><Download size={13} />Save As…</button>
        </div>
      </div>
    </li>
  {/each}
</ul>

<style>
  .gallery {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: 10px;
  }
  .gallery li {
    display: flex;
    gap: 10px;
    align-items: flex-start;
  }
  img,
  .icon {
    width: 72px;
    height: 72px;
    object-fit: cover;
    border-radius: 6px;
    border: 1px solid var(--border);
    display: grid;
    place-items: center;
    flex: none;
  }
  .meta {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }
  .meta strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .small {
    font-size: 12.5px;
  }
</style>
