// One dispatcher for native menu items and keyboard shortcuts so both behave identically.
import { app } from "./state.svelte";
import type { Screen } from "./state.svelte";
import { api, toAppError } from "./api";
import { confirm } from "./confirm.svelte";

export const isMac = () => /mac/i.test(navigator.platform) || app.platform === "macos";
export const modKey = () => (isMac() ? "⌘" : "Ctrl+");

export const SHORTCUTS: { keys: string; action: string; label: string }[] = [
  { keys: "N", action: "new_entry", label: "New entry" },
  { keys: "Enter", action: "save_entry", label: "Save Entry / Save Changes" },
  { keys: "F", action: "find", label: "Find in Traffic Log" },
  { keys: "E", action: "export", label: "Open Export" },
  { keys: "1", action: "nav_dashboard", label: "Dashboard" },
  { keys: "2", action: "nav_quick_entry", label: "Quick Entry" },
  { keys: "3", action: "nav_traffic_log", label: "Traffic Log" },
  { keys: "4", action: "nav_messages", label: "Messages" },
  { keys: "5", action: "nav_reports", label: "Reports & Export" },
  { keys: ",", action: "settings", label: "Settings" },
  { keys: "Shift+O", action: "logbooks", label: "Logbooks" },
  { keys: "Shift+B", action: "backup_now", label: "Back Up Now" },
  { keys: "/", action: "help_shortcuts", label: "Keyboard shortcuts (or F1)" },
];

const BOOK_ACTIONS = new Set([
  "new_entry",
  "save_entry",
  "find",
  "export",
  "nav_dashboard",
  "nav_quick_entry",
  "nav_traffic_log",
  "nav_messages",
  "nav_reports",
  "backup_now",
  "close_logbook",
]);
const DIALOG_SAFE = new Set(["save_entry", "help_shortcuts"]);

let lastAction = { id: "", at: 0 };

export async function runAction(id: string, source: "menu" | "key" = "key"): Promise<void> {
  const now = performance.now();
  // A native accelerator may also reach the webview as a keydown; run it once.
  if (lastAction.id === id && now - lastAction.at < 350) return;
  lastAction = { id, at: now };

  if (app.screen === "wizard" && id !== "help_shortcuts" && id !== "quit") {
    app.announce("Finish first-run setup first", true);
    return;
  }
  if ((app.modalDepth > 0 || confirm.current) && !DIALOG_SAFE.has(id)) {
    app.announce(
      "That shortcut is not available while a dialog is open. Press Esc to close the dialog.",
      true,
    );
    return;
  }
  if (BOOK_ACTIONS.has(id) && !app.active) {
    app.announce("Open a logbook first", true);
    if (source === "menu") app.toast("info", "Open or create a logbook first.");
    return;
  }
  const nav = (s: Screen) => app.navigate(s);
  switch (id) {
    case "new_entry":
      app.editorRequest = { mode: "new" };
      await nav("quick");
      return;
    case "save_entry": {
      // Inside a dialog, Ctrl/Cmd+Enter confirms that dialog; it must never save a form behind it.
      const dialogs = document.querySelectorAll<HTMLElement>("[role=dialog]");
      const top = dialogs[dialogs.length - 1];
      const ed = app.topEditor;
      if (top) {
        const edRoot = ed?.root();
        if (ed && edRoot && top.contains(edRoot)) ed.save();
        else {
          const primary = top.querySelector<HTMLButtonElement>(
            "footer button.primary:not(:disabled), footer button.success:not(:disabled)",
          );
          if (primary) primary.click();
          else app.announce("Nothing to save in this dialog", true);
        }
        return;
      }
      if (ed) ed.save();
      else app.announce("No entry form is open to save", true);
      return;
    }
    case "find":
      await nav("log");
      setTimeout(() => app.focusSearch?.(), 30);
      return;
    case "export":
      await nav("reports");
      return;
    case "nav_dashboard":
      return nav("dashboard");
    case "nav_quick_entry":
      return nav("quick");
    case "nav_traffic_log":
      return nav("log");
    case "nav_messages":
      return nav("messages");
    case "nav_reports":
      return nav("reports");
    case "settings":
      return nav("settings");
    case "logbooks":
      return nav("home");
    case "close_logbook":
      await closeActiveLogbook();
      return;
    case "backup_now":
      await backupNow();
      return;
    case "import_package":
      await nav("home");
      app.dialog = "import";
      return;
    case "restore_backup":
      await nav("home");
      app.dialog = "restore";
      return;
    case "toggle_theme":
      await toggleTheme();
      return;
    case "help_shortcuts":
      app.dialog = "shortcuts";
      return;
    case "help_data":
      app.dialog = "data";
      return;
    case "help_about":
      app.dialog = "about";
      return;
  }
}

export async function toggleTheme() {
  if (!app.settings) return;
  const next = app.settings.theme === "light" ? "dark" : "light";
  try {
    app.settings = await api.saveSettings({ ...app.settings, theme: next });
  } catch (e) {
    app.toast("error", toAppError(e).message);
  }
}

export async function backupNow() {
  app.announce("Backup started");
  const o = await api
    .runBackupNow()
    .catch((e) => ({ ok: false, skipped: false, path: null, error: toAppError(e) }));
  if (o.ok) app.toast("ok", "Backup Succeeded: verified copy written.");
  else app.toast("error", `Backup Failed: ${o.error?.message ?? "unknown error"}`);
  if (app.active) app.backup = await api.getBackupStatus().catch(() => app.backup);
}

/**
 * Close the open logbook: flush any draft, confirm if unsaved entry content remains, run the
 * close backup, and require an explicit decision if that backup fails. Returns true if closed.
 */
export async function closeActiveLogbook(): Promise<boolean> {
  if (!app.active) return true;
  await app.flushDrafts();
  // Unsaved text may be in an open form or only in drafts left from earlier (after navigating away).
  const drafts = await api.listDrafts().catch(() => []);
  if (app.editorDirty || drafts.length > 0) {
    const n = Math.max(drafts.length, app.editorDirty ? 1 : 0);
    const r = await confirm.ask({
      title: n === 1 ? "Unsaved entry" : `${n} unsaved entries`,
      message:
        n === 1
          ? "An entry you were typing has not been saved to the logbook."
          : `${n} entries you were typing have not been saved to the logbook.`,
      detail:
        app.draftStatus.kind === "failed"
          ? `Its recovery draft could NOT be stored: ${app.draftStatus.error.message}. Closing now loses it.`
          : n === 1
            ? "It is kept as a recoverable draft and offered the next time you open this logbook."
            : "They are kept as recoverable drafts and offered the next time you open this logbook.",
      buttons: [
        { id: "cancel", label: "Go back", tone: "primary" },
        {
          id: "close",
          label: app.draftStatus.kind === "failed" ? "Close and lose it" : "Close, keep draft",
          tone: "danger",
        },
      ],
      cancelId: "cancel",
    });
    if (r.id !== "close") return false;
  }
  app.toast("info", "Closing logbook: running close-time backup…", 3000);
  for (;;) {
    let res;
    try {
      res = await api.closeLogbook(true);
    } catch (e) {
      app.toast("error", toAppError(e).message);
      return false;
    }
    if (res.closed) {
      if (res.backup?.ok && !res.backup.skipped) app.toast("ok", "Close backup succeeded.");
      break;
    }
    const r = await confirm.ask({
      title: "Close backup failed",
      message: res.backup?.error?.message ?? "The close-time backup could not be completed.",
      detail: "Your entries are safe in the live logbook. Earlier backups were not changed.",
      buttons: [
        { id: "retry", label: "Retry backup", tone: "primary" },
        { id: "skip", label: "Close without backup", tone: "danger" },
        { id: "cancel", label: "Stay open" },
      ],
      cancelId: "cancel",
    });
    if (r.id === "cancel") return false;
    if (r.id === "skip") {
      await api.closeLogbook(false);
      break;
    }
  }
  app.setActive(null);
  app.saveStatus = { kind: "idle" };
  app.draftStatus = { kind: "idle" };
  app.selection = [];
  app.screen = "home";
  return true;
}

let quitting = false;
export async function handleCloseRequested() {
  if (quitting) return;
  quitting = true;
  try {
    const closed = await closeActiveLogbook();
    if (closed) {
      await api.quitApp();
      return;
    }
    await api.cancelClose();
  } finally {
    quitting = false;
  }
}

/** Map a keydown to an action id (Ctrl on Windows/Linux, Cmd on macOS). */
export function keyToAction(e: KeyboardEvent): string | null {
  if (e.key === "F1" && !e.metaKey && !e.ctrlKey && !e.altKey) return "help_shortcuts";
  const mod = isMac() ? e.metaKey : e.ctrlKey;
  if (!mod || e.altKey) return null;
  const k = e.key;
  if (e.shiftKey) {
    if (k.toLowerCase() === "o") return "logbooks";
    if (k.toLowerCase() === "b") return "backup_now";
    return null;
  }
  switch (k.toLowerCase()) {
    case "n":
      return "new_entry";
    case "enter":
      return "save_entry";
    case "f":
      return "find";
    case "e":
      return "export";
    case "1":
      return "nav_dashboard";
    case "2":
      return "nav_quick_entry";
    case "3":
      return "nav_traffic_log";
    case "4":
      return "nav_messages";
    case "5":
      return "nav_reports";
    case ",":
      return "settings";
    case "/":
      return "help_shortcuts";
  }
  return null;
}
