import type * as T from "./types";
import { api, toAppError } from "./api";
import { todayBoundsUtc } from "./time";
import type { Clock, Zone } from "./time";

export type Screen = "wizard" | "home" | "dashboard" | "quick" | "log" | "messages" | "reports" | "settings";

export type SaveStatus =
  | { kind: "idle" }
  | { kind: "saving" }
  | { kind: "saved"; seq: number; at: string; edit: boolean; duplicate: boolean }
  | { kind: "failed"; error: T.AppError }
  | { kind: "invalid"; message: string };

export type DraftStatus =
  | { kind: "idle" }
  | { kind: "pending" }
  | { kind: "saved"; at: string }
  | { kind: "failed"; error: T.AppError };

export interface Toast {
  id: number;
  tone: "ok" | "warn" | "error" | "info";
  text: string;
}

export type EditorRequest =
  | { mode: "new" }
  | { mode: "template"; template: T.EntryTemplate }
  | { mode: "copy"; entry: T.Entry }
  | { mode: "draft"; draft: T.Draft };

export interface EditorHandle {
  id: number;
  save: () => void;
  flush: () => Promise<void>;
  dirty: () => boolean;
  root: () => HTMLElement | undefined;
}

export function defaultQuery(): T.EntryQuery {
  return {
    text: "",
    from_utc: null,
    to_utc: null,
    kinds: [],
    directions: [],
    priorities: [],
    handling: [],
    station: "",
    tags: [],
    status: "active",
    favorites_only: false,
    flagged_only: false,
    ids: [],
    sort: "event",
    descending: true,
    limit: 50,
    offset: 0,
  };
}

const STORAGE_FAULTS = new Set(["STORAGE_UNAVAILABLE", "DISK_FULL", "PERMISSION_DENIED", "IO", "INTEGRITY"]);

class AppStore {
  settings = $state<T.GlobalSettings | null>(null);
  profiles = $state<T.Profile[]>([]);
  presets = $state<T.Preset[]>([]);
  appVersion = $state("");
  dataDir = $state("");
  platform = $state("");
  active = $state<T.ActiveView | null>(null);
  screen = $state<Screen>("home");
  saveStatus = $state<SaveStatus>({ kind: "idle" });
  draftStatus = $state<DraftStatus>({ kind: "idle" });
  storageBlocked = $state<T.AppError | null>(null);
  backup = $state<T.BackupStatus | null>(null);
  toasts = $state<Toast[]>([]);
  politeMessage = $state("");
  assertiveMessage = $state("");
  modalDepth = $state(0);
  dataVersion = $state(0);
  editorRequest = $state<EditorRequest | null>(null);
  editEntryId = $state<string | null>(null);
  detailEntryId = $state<string | null>(null);
  logQuery = $state<T.EntryQuery>(defaultQuery());
  selection = $state<string[]>([]);
  pendingRecovery = $state<T.Draft[]>([]);
  /** A recovered draft of an edit, picked up by EntryDetail for that entry. */
  editDraft = $state<T.Draft | null>(null);
  dialog = $state<string | null>(null);
  reportScope = $state<"all" | "range" | "filter">("all");
  /** Operational periods of the open logbook (V2), ordered by start. */
  periods = $state<T.OperationalPeriod[]>([]);
  /** Channel plans by period id, loaded on demand. */
  plans = $state<Record<string, T.ChannelPlan>>({});

  /** Mounted entry editors, innermost (most recently mounted) last. */
  private editors: EditorHandle[] = [];
  private editorSeq = 0;
  editorDirty = $state(false);
  focusSearch: (() => void) | null = null;
  private toastSeq = 0;

  zone: Zone = $derived(
    (this.active?.settings.time_display_override ?? this.settings?.time_display ?? "local") as Zone,
  );
  clock: Clock = $derived((this.settings?.clock ?? "24h") as Clock);
  profile: T.Profile | null = $derived(
    this.profiles.find((p) => p.id === this.settings?.active_profile_id) ?? null,
  );
  bookType: T.LogbookType | null = $derived(this.active?.info.logbook_type ?? null);
  readOnly = $derived(this.active?.info.read_only ?? false);

  applyBootstrap(b: T.Bootstrap) {
    this.settings = b.settings;
    this.profiles = b.profiles;
    this.presets = b.presets;
    this.appVersion = b.app_version;
    this.dataDir = b.data_dir;
    this.platform = b.platform;
    this.setActive(b.active);
  }

  setActive(v: T.ActiveView | null) {
    const changed = v?.info.id !== this.active?.info.id;
    this.active = v;
    if (changed) {
      this.plans = {};
      void this.refreshPeriods();
    }
    this.backup = v?.backup ?? null;
    this.storageBlocked = v && !v.health.ok && v.health.error ? v.health.error : null;
    this.dataVersion++;
  }

  stats = $state<T.LogbookStats | null>(null);

  async refreshStats() {
    if (!this.active) {
      this.stats = null;
      return;
    }
    const [a, b] = todayBoundsUtc(this.zone);
    try {
      this.stats = await api.logbookStats(a, b);
    } catch {
      this.stats = null;
    }
  }

  async refreshPeriods() {
    if (!this.active) {
      this.periods = [];
      return;
    }
    try {
      this.periods = await api.listPeriods();
    } catch {
      this.periods = [];
    }
  }

  async loadPlan(opId: string): Promise<T.ChannelPlan | null> {
    if (this.plans[opId]) return this.plans[opId];
    try {
      const p = await api.periodPlan(opId);
      this.plans = { ...this.plans, [opId]: p };
      return p;
    } catch {
      return null;
    }
  }

  async refreshActive() {
    try {
      this.setActive(await api.getActive());
    } catch (e) {
      this.toast("error", toAppError(e).message);
    }
  }

  async refreshProfilesPresets() {
    const [profiles, presets] = await Promise.all([api.listProfiles(), api.listPresets()]);
    this.profiles = profiles;
    this.presets = presets;
  }

  announce(text: string, assertive = false) {
    if (assertive) {
      this.assertiveMessage = "";
      queueMicrotask(() => (this.assertiveMessage = text));
    } else {
      this.politeMessage = "";
      queueMicrotask(() => (this.politeMessage = text));
    }
  }

  toast(tone: Toast["tone"], text: string, ms = 6000) {
    const id = ++this.toastSeq;
    this.toasts = [...this.toasts, { id, tone, text }];
    this.announce(text, tone === "error");
    setTimeout(() => this.dismissToast(id), tone === "error" ? Math.max(ms, 10000) : ms);
  }

  dismissToast(id: number) {
    this.toasts = this.toasts.filter((t) => t.id !== id);
  }

  /** Record a final-save failure; storage faults block further final saves until rechecked. */
  noteSaveFailure(err: T.AppError) {
    if (err.code === "VALIDATION") {
      // Nothing is wrong with storage: the entry needs correcting, not the drive.
      this.saveStatus = { kind: "invalid", message: err.message };
      this.announce(`Not saved: ${err.message}`, true);
      return;
    }
    this.saveStatus = { kind: "failed", error: err };
    if (STORAGE_FAULTS.has(err.code)) this.storageBlocked = err;
    this.announce(`Save Failed: ${err.message}`, true);
  }

  async recheckStorage(): Promise<boolean> {
    try {
      const h = await api.storageHealth();
      if (this.active) this.active = { ...this.active, health: h };
      this.storageBlocked = h.ok
        ? null
        : (h.error ?? { code: "STORAGE_UNAVAILABLE", message: "Storage check failed" });
      this.announce(
        h.ok
          ? "Logbook storage is available again"
          : `Storage still unavailable: ${this.storageBlocked?.message}`,
        !h.ok,
      );
      return h.ok;
    } catch (e) {
      this.storageBlocked = toAppError(e);
      return false;
    }
  }

  registerEditor(e: Omit<EditorHandle, "id">): () => void {
    const id = ++this.editorSeq;
    this.editors.push({ ...e, id });
    this.updateDirty();
    return () => {
      this.editors = this.editors.filter((x) => x.id !== id);
      this.updateDirty();
    };
  }

  get topEditor(): EditorHandle | null {
    return this.editors[this.editors.length - 1] ?? null;
  }

  updateDirty() {
    this.editorDirty = this.editors.some((e) => e.dirty());
  }

  /** Persist every open editor's unsaved text as a recovery draft. */
  async flushDrafts() {
    await Promise.all(this.editors.map((e) => e.flush()));
  }

  async navigate(screen: Screen) {
    if (screen === this.screen) return;
    await this.flushDrafts();
    this.screen = screen;
  }

  bumpData() {
    this.dataVersion++;
  }
}

export const app = new AppStore();
