// Android integration for the web UI: the Back button, leaving/returning to the foreground,
// system-bar colours, and reopening the last logbook after Android ends the process.
// EmcommPlugin.kt calls window.__emcommBack() and window.__emcommLifecycle(event).
import { app } from "./state.svelte";
import { api, toAppError } from "./api";
import { confirm } from "./confirm.svelte";
import { handleCloseRequested } from "./actions";

const LAST_BOOK_KEY = "emcomm.lastOpenLogbook";

declare global {
  interface Window {
    __emcommBack?: () => string;
    __emcommLifecycle?: (event: string) => void;
  }
}

let exitAsked = false;

async function askExit() {
  if (exitAsked) return;
  exitAsked = true;
  try {
    const r = await confirm.ask({
      title: "Exit EMCOMM LOG?",
      message: app.active
        ? "The logbook will be closed and backed up. Saved entries are never lost by exiting."
        : "Saved entries are never lost by exiting.",
      buttons: [
        { id: "stay", label: "Stay", tone: "primary" },
        { id: "exit", label: "Exit" },
      ],
      cancelId: "stay",
    });
    if (r.id === "exit") await handleCloseRequested();
  } finally {
    exitAsked = false;
  }
}

/** Back: close the top dialog or panel, then return to the main screen, then offer to exit. */
function onBack(): string {
  if (app.modalDepth > 0 || confirm.current) {
    document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    return "handled";
  }
  if (app.detailEntryId) {
    app.detailEntryId = null;
    return "handled";
  }
  if (app.screen === "wizard") return "none";
  const root = app.active ? "dashboard" : "home";
  if (app.screen !== root) {
    void app.navigate(root);
    return "handled";
  }
  void askExit();
  return "handled";
}

let backgroundRun: Promise<void> | null = null;

/** Leaving the foreground: Android may end the process at any time without a close event. */
function onStop() {
  if (backgroundRun) return;
  backgroundRun = (async () => {
    try {
      await app.flushDrafts();
      const out = await api.appBackgrounded();
      if (!out.ok && out.error) app.toast("error", `Background backup failed: ${out.error.message}`);
      if (app.active) app.backup = await api.getBackupStatus().catch(() => app.backup);
    } catch (e) {
      app.toast("error", toAppError(e).message);
    } finally {
      backgroundRun = null;
    }
  })();
}

async function onResume() {
  if (!app.active) return;
  // A microSD card may have been removed or inserted while away.
  app.backup = await api.getBackupStatus().catch(() => app.backup);
  try {
    const h = await api.storageHealth();
    app.storageBlocked = h.ok ? null : (h.error ?? null);
  } catch {
    /* keep the previous state */
  }
}

export function installAndroidShell() {
  window.__emcommBack = onBack;
  window.__emcommLifecycle = (event: string) => {
    if (event === "stop") onStop();
    else if (event === "resume") void onResume();
  };
}

/** Match the status/navigation bar area to the current theme. */
export function syncSystemBars(dark: boolean) {
  const bg = getComputedStyle(document.body).backgroundColor;
  const hex = toHex(bg) ?? (dark ? "#08131F" : "#F4F6F9");
  void api.setSystemBars(dark, hex).catch(() => {});
}

function toHex(rgb: string): string | null {
  const m = rgb.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
  if (!m) return null;
  return "#" + [m[1], m[2], m[3]].map((n) => Number(n).toString(16).padStart(2, "0")).join("");
}

function storage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/** Remember the open logbook so it can be reopened if Android ends the app in the background. */
export function rememberOpenLogbook(id: string | null) {
  const s = storage();
  try {
    if (id) s?.setItem(LAST_BOOK_KEY, id);
    else s?.removeItem(LAST_BOOK_KEY);
  } catch {
    /* convenience only */
  }
}

/** On a cold start with no logbook open, reopen the one that was open when the app last ran. */
export async function reopenLastLogbook(): Promise<boolean> {
  let id: string | null = null;
  try {
    id = storage()?.getItem(LAST_BOOK_KEY) ?? null;
  } catch {
    id = null;
  }
  if (!id || app.active) return false;
  try {
    const rows = await api.listLogbooks();
    if (!rows.some((r) => r.id === id && r.available && !r.archived)) return false;
    const view = await api.openLogbook(id);
    app.setActive(view);
    app.toast("ok", `Reopened ${view.info.name}`);
    return true;
  } catch (e) {
    app.toast("info", `Could not reopen the last logbook: ${toAppError(e).message}`);
    return false;
  }
}
