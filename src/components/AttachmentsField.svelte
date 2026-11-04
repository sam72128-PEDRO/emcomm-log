<script lang="ts">
  // 2.2 attachments (ATT-*, DESK-ATT-*): pick, drag and drop, or paste into this box. Files are copied into
  // the logbook's staging area at once (so the source may vanish) and become part of the entry only when it
  // is saved. Nothing is opened automatically.
  import { onMount } from "svelte";
  import { Paperclip, FileText, X, Upload, Undo2, LoaderCircle } from "@lucide/svelte";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { app } from "../lib/state.svelte";
  import { api, newId, toAppError } from "../lib/api";
  import { confirm } from "../lib/confirm.svelte";
  import type * as T from "../lib/types";

  let {
    staged = $bindable(),
    existing = [],
    removed = $bindable(),
    captions = $bindable(),
    disabled = false,
    id,
  }: {
    staged: T.StagedAttachment[];
    /** Attachments already saved on the entry (editing). */
    existing?: T.Attachment[];
    removed: string[];
    captions: Record<string, string>;
    disabled?: boolean;
    id: string;
  } = $props();

  const LARGE = 25 * 1024 * 1024;
  let zone: HTMLDivElement | undefined = $state();
  let over = $state(false);
  let copying = $state<{ token: string; name: string; done: number; total: number } | null>(null);
  let thumbs = $state<Record<string, string | null>>({});

  const kb = (n: number) =>
    n >= 1024 * 1024 ? `${(n / 1024 / 1024).toFixed(1)} MB` : `${Math.ceil(n / 1024)} KB`;
  const isImage = (mime?: string) => !!mime && /^image\/(jpeg|png|gif|webp)$/.test(mime);

  async function thumb(key: string, attachmentId: string | null, stagingId: string | null) {
    if (key in thumbs) return;
    thumbs[key] = null;
    try {
      thumbs[key] = await api.attachmentThumbnail(attachmentId, stagingId);
    } catch {
      thumbs[key] = null;
    }
  }
  $effect(() => {
    for (const a of staged) if (isImage(a.mime)) void thumb(`s:${a.staging_id}`, null, a.staging_id);
    for (const a of existing) if (isImage(a.mime)) void thumb(`a:${a.id}`, a.id, null);
  });

  async function stageFiles(files: T.PickedFile[], source: T.AttachmentSource) {
    for (const f of files) {
      if (f.size_bytes >= LARGE) {
        const a = await confirm.ask({
          title: "Large file",
          message: `${f.name} is ${kb(f.size_bytes)}. Attach it anyway?`,
          detail:
            "It is copied into the logbook folder, every backup and every .emlog package. Make sure the drive has room.",
          buttons: [
            { id: "skip", label: "Skip it" },
            { id: "attach", label: "Attach", tone: "primary" },
          ],
          cancelId: "skip",
        });
        if (a.id !== "attach") continue;
      }
      const token = newId();
      copying = { token, name: f.name, done: 0, total: f.size_bytes };
      try {
        const s = await api.stageAttachmentPath(f.path, source, token);
        staged = [...staged, s];
        app.announce(`Attached ${s.display_name}`);
      } catch (e) {
        const err = toAppError(e);
        if (err.code !== "CANCELLED") app.toast("error", `${f.name} not attached: ${err.message}`);
        else app.toast("info", `${f.name}: copy cancelled; nothing attached.`);
      } finally {
        copying = null;
      }
    }
  }

  async function pick() {
    try {
      await stageFiles(await api.pickAttachments(), "file");
    } catch (e) {
      app.toast("error", `Attachment failed: ${toAppError(e).message}`);
    }
  }

  // Paste only into this box, so ordinary text paste in the message or notes is never intercepted.
  async function onPaste(e: ClipboardEvent) {
    // This box only takes files; text pasted here is ignored (it belongs in the message or notes).
    e.preventDefault();
    const items = [...(e.clipboardData?.items ?? [])].filter((i) => i.kind === "file");
    if (!items.length) {
      app.toast("info", "Nothing to attach: the clipboard holds no image or file.");
      return;
    }
    for (const it of items) {
      const file = it.getAsFile();
      if (!file) continue;
      const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
      const ext =
        file.type === "image/jpeg" ? "jpg" : file.type.startsWith("image/") ? file.type.slice(6) : "bin";
      const name = file.name && file.name !== "image.png" ? file.name : `Pasted image ${stamp}.${ext}`;
      try {
        const s = await api.stageAttachmentBytes(new Uint8Array(await file.arrayBuffer()), name, "paste");
        staged = [...staged, s];
        app.announce(`Pasted ${s.display_name}`);
      } catch (err) {
        app.toast("error", `Paste not attached: ${toAppError(err).message}`);
      }
    }
  }

  function unstage(a: T.StagedAttachment) {
    staged = staged.filter((x) => x.staging_id !== a.staging_id);
    api.discardStaged(a.staging_id).catch(() => {});
  }

  function inZone(x: number, y: number) {
    const r = zone?.getBoundingClientRect();
    const k = window.devicePixelRatio || 1;
    return !!r && x / k >= r.left && x / k <= r.right && y / k >= r.top && y / k <= r.bottom;
  }

  onMount(() => {
    const offs: (() => void)[] = [];
    let alive = true;
    listen<T.AttachProgress>("attach-progress", (ev) => {
      if (copying && ev.payload.token === copying.token)
        copying = { ...copying, done: ev.payload.done, total: ev.payload.total };
    }).then((u) => (alive ? offs.push(u) : u()));
    try {
      getCurrentWebview()
        .onDragDropEvent((ev) => {
          const p = ev.payload;
          if (disabled || !zone || zone.offsetParent === null) return;
          if (p.type === "over" || p.type === "enter") over = inZone(p.position.x, p.position.y);
          else if (p.type === "leave") over = false;
          else if (p.type === "drop") {
            const hit = inZone(p.position.x, p.position.y);
            over = false;
            if (hit && p.paths.length)
              void api.describeFiles(p.paths).then((f) => stageFiles(f, "drag_drop"));
          }
        })
        .then((u) => (alive ? offs.push(u) : u()));
    } catch {
      /* not running inside Tauri (tests) */
    }
    return () => {
      alive = false;
      offs.forEach((u) => u());
    };
  });
</script>

<div class="atts-field">
  <div
    bind:this={zone}
    class="drop"
    class:over
    role="group"
    aria-label="Attachments"
    aria-describedby="{id}-hint"
  >
    <Upload size={16} aria-hidden="true" />
    <span>Drop files here</span>
    <input
      class="paste"
      type="text"
      id="{id}-paste"
      aria-label="Paste a screenshot or image here"
      placeholder="or click here and paste an image"
      value=""
      onpaste={onPaste}
      oninput={(e) => (e.currentTarget.value = "")}
      {disabled}
    />
    <span class="spacer"></span>
    <button type="button" class="small" onclick={pick} {disabled}><Paperclip size={14} />Attach files…</button
    >
  </div>
  <span class="hint" id="{id}-hint"
    >Copied into this logbook at once; added to the entry when you save it. Nothing is opened automatically.</span
  >
  {#if copying}
    <div class="row copying" role="status" aria-live="polite">
      <LoaderCircle size={14} class="spin" />
      <span
        >Copying {copying.name}… {copying.total ? Math.round((copying.done / copying.total) * 100) : 0}%</span
      >
      <progress max={copying.total || 1} value={copying.done}></progress>
      <button type="button" class="small" onclick={() => api.cancelAttach()}>Cancel</button>
    </div>
  {/if}

  {#if existing.length || staged.length}
    <ul class="att-list">
      {#each existing as a (a.id)}
        {@const gone = removed.includes(a.id)}
        <li class:gone>
          {#if thumbs[`a:${a.id}`]}<img src={thumbs[`a:${a.id}`]} alt="" />{:else}<span class="icon"
              ><FileText size={22} /></span
            >{/if}
          <div class="meta">
            <strong>{a.display_name}</strong>
            <span class="muted small">{kb(a.size_bytes)}{gone ? " · will be removed when you save" : ""}</span
            >
            <input
              type="text"
              aria-label="Caption for {a.display_name}"
              placeholder="Caption (optional)"
              maxlength="200"
              disabled={gone || disabled}
              value={captions[a.id] ?? a.caption ?? ""}
              oninput={(e) => {
                const v = e.currentTarget.value;
                const next = { ...captions };
                if (v === (a.caption ?? "")) delete next[a.id];
                else next[a.id] = v;
                captions = next;
              }}
            />
          </div>
          {#if gone}<button
              type="button"
              class="small"
              onclick={() => (removed = removed.filter((x) => x !== a.id))}><Undo2 size={14} />Keep</button
            >{:else}<button
              type="button"
              class="small"
              {disabled}
              onclick={() => (removed = [...removed, a.id])}
              aria-label="Remove {a.display_name}"><X size={14} />Remove</button
            >{/if}
        </li>
      {/each}
      {#each staged as a, i (a.staging_id)}
        <li>
          {#if thumbs[`s:${a.staging_id}`]}<img src={thumbs[`s:${a.staging_id}`]} alt="" />{:else}<span
              class="icon"><FileText size={22} /></span
            >{/if}
          <div class="meta">
            <strong>{a.display_name}</strong>
            <span class="muted small"
              >{kb(a.size_bytes)} · new{a.source === "paste"
                ? " (pasted)"
                : a.source === "drag_drop"
                  ? " (dropped)"
                  : ""}</span
            >
            <input
              type="text"
              aria-label="Caption for {a.display_name}"
              placeholder="Caption (optional)"
              maxlength="200"
              bind:value={staged[i].caption}
            />
          </div>
          <button type="button" class="small" onclick={() => unstage(a)} aria-label="Remove {a.display_name}"
            ><X size={14} />Remove</button
          >
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .atts-field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .drop {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border: 1.5px dashed var(--border-strong);
    border-radius: 8px;
    color: var(--text-2);
    font-size: 13px;
  }
  .paste {
    max-width: 260px;
    font-size: 12.5px;
  }
  .drop.over {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }
  .copying progress {
    flex: 1;
    max-width: 220px;
  }
  .att-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .att-list li {
    display: flex;
    gap: 10px;
    align-items: center;
  }
  .att-list li.gone {
    opacity: 0.55;
  }
  .att-list img,
  .att-list .icon {
    width: 56px;
    height: 56px;
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
    gap: 2px;
    flex: 1;
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
