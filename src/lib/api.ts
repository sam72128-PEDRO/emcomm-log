// Typed bridge to the Rust commands. Every authoritative operation goes through here.
import { invoke } from "@tauri-apps/api/core";
import type * as T from "./types";

export function isAppError(e: unknown): e is T.AppError {
  return typeof e === "object" && e !== null && "code" in e && "message" in e;
}

export function toAppError(e: unknown): T.AppError {
  if (isAppError(e)) return e;
  return { code: "INTERNAL", message: e instanceof Error ? e.message : String(e) };
}

async function call<R>(cmd: string, args?: Record<string, unknown>): Promise<R> {
  try {
    return await invoke<R>(cmd, args);
  } catch (e) {
    throw toAppError(e);
  }
}

export interface DialogFilter {
  name: string;
  extensions: string[];
}

export const api = {
  getBootstrap: () => call<T.Bootstrap>("get_bootstrap"),
  saveSettings: (settings: T.GlobalSettings) => call<T.GlobalSettings>("save_settings", { settings }),
  checkFolder: (path: string, purpose: "live" | "backup") =>
    call<T.FolderCheck>("check_folder", { path, purpose }),
  pickFolder: (title: string, start?: string | null) =>
    call<string | null>("pick_folder", { title, start: start ?? null }),
  pickFile: (title: string, filters: DialogFilter[]) => call<string | null>("pick_file", { title, filters }),
  pickSaveFile: (title: string, defaultName: string, filters: DialogFilter[], start?: string | null) =>
    call<string | null>("pick_save_file", { title, defaultName, filters, start: start ?? null }),

  createProfile: (input: T.ProfileInput) => call<T.Profile>("create_profile", { input }),
  updateProfile: (id: string, input: T.ProfileInput) => call<T.Profile>("update_profile", { id, input }),
  setProfileActive: (id: string, active: boolean) => call<T.Profile>("set_profile_active", { id, active }),
  listProfiles: () => call<T.Profile[]>("list_profiles"),
  listPresets: () => call<T.Preset[]>("list_presets"),
  createPreset: (input: T.PresetInput) => call<T.Preset>("create_preset", { input }),
  updatePreset: (id: string, input: T.PresetInput) => call<T.Preset>("update_preset", { id, input }),
  duplicatePreset: (id: string) => call<T.Preset>("duplicate_preset", { id }),
  deletePreset: (id: string) => call<void>("delete_preset", { id }),

  listLogbooks: () => call<T.LogbookIndexRow[]>("list_logbooks"),
  createLogbook: (req: T.CreateLogbookRequest) => call<T.ActiveView>("create_logbook", { req }),
  openLogbook: (id: string) => call<T.ActiveView>("open_logbook", { id }),
  openLogbookFolder: (path: string) => call<T.ActiveView>("open_logbook_folder", { path }),
  closeLogbook: (backup: boolean) => call<T.CloseResult>("close_logbook", { backup }),
  relinkLogbook: (id: string, path: string) => call<T.LogbookIndexRow[]>("relink_logbook", { id, path }),
  forgetLogbook: (id: string) => call<T.LogbookIndexRow[]>("forget_logbook", { id }),
  setLogbookArchived: (id: string, archived: boolean) =>
    call<T.LogbookIndexRow[]>("set_logbook_archived", { id, archived }),
  renameLogbook: (id: string, name: string) => call<T.LogbookIndexRow[]>("rename_logbook", { id, name }),
  updateLogbookIdentity: (name: string, incident: T.IncidentFields) =>
    call<T.LogbookInfo>("update_logbook_identity", { name, incident }),
  updateLogbookSettings: (timeDisplayOverride: string | null, operationalNotes: string) =>
    call<T.LogbookSettings>("update_logbook_settings", { timeDisplayOverride, operationalNotes }),
  getActive: () => call<T.ActiveView | null>("get_active"),
  logbookStats: (dayStartUtc: string | null, dayEndUtc: string | null) =>
    call<T.LogbookStats>("logbook_stats", { dayStartUtc, dayEndUtc }),
  storageHealth: () => call<T.StorageHealth>("storage_health"),
  revealPath: (path: string) => call<void>("reveal_path", { path }),
  openAttachment: (attachmentId: string) => call<void>("open_attachment", { attachmentId }),
  openExport: (path: string) => call<void>("open_export", { path }),

  saveEntry: (input: T.EntryInput, requestId: string, draftId: string | null) =>
    call<T.SaveResult>("save_entry", { input, requestId, draftId }),
  saveChanges: (
    entryId: string,
    baseRevision: number,
    input: T.EntryInput,
    reason: string,
    requestId: string,
    draftId: string | null,
  ) => call<T.SaveResult>("save_changes", { entryId, baseRevision, input, reason, requestId, draftId }),
  voidEntry: (entryId: string, reason: string, requestId: string) =>
    call<T.SaveResult>("void_entry", { entryId, reason, requestId }),
  restoreEntry: (entryId: string, reason: string, requestId: string) =>
    call<T.SaveResult>("restore_entry", { entryId, reason, requestId }),
  getEntry: (id: string) => call<T.Entry>("get_entry", { id }),
  listEntries: (query: T.EntryQuery) => call<T.EntryPage>("list_entries", { query }),
  entryRevisions: (id: string) => call<T.EntryRevision[]>("entry_revisions", { id }),
  setFavorite: (id: string, favorite: boolean) => call<T.Entry>("set_favorite", { id, favorite }),
  stageAttachment: () => call<T.StagedAttachment | null>("stage_attachment"),
  discardStaged: (stagingId: string) => call<void>("discard_staged", { stagingId }),
  stagedExists: (stagingIds: string[]) => call<string[]>("staged_exists", { stagingIds }),
  listTemplates: () => call<T.EntryTemplate[]>("list_templates"),
  saveTemplate: (id: string | null, name: string, defaults: T.TemplateDefaults) =>
    call<T.EntryTemplate>("save_template", { id, name, defaults }),
  deleteTemplate: (id: string) => call<void>("delete_template", { id }),

  saveDraft: (meta: T.DraftMeta, content: unknown) => call<string>("save_draft", { meta, content }),
  listDrafts: () => call<T.Draft[]>("list_drafts"),
  deleteDraft: (logbookId: string, draftId: string) => call<void>("delete_draft", { logbookId, draftId }),

  runBackupNow: () => call<T.BackupOutcome>("run_backup_now"),
  getBackupStatus: () => call<T.BackupStatus>("get_backup_status"),
  listBackups: (destination: string | null) => call<T.BackupListing[]>("list_backups", { destination }),
  previewBackup: (path: string) => call<T.BackupPreview>("preview_backup", { path }),
  restoreBackup: (path: string, parentDir: string, name: string | null) =>
    call<T.LogbookIndexRow>("restore_backup", { path, parentDir, name }),

  exportPackage: (id: string | null, dest: string) =>
    call<T.ExportPackageResult>("export_package", { id, dest }),
  previewPackage: (path: string) => call<T.PackagePreview>("preview_package", { path }),
  importPackage: (path: string, parentDir: string, name: string | null) =>
    call<T.ImportOutcome>("import_package", { path, parentDir, name }),

  exportPreview: (req: T.ExportRequest) => call<T.ExportPreview>("export_preview", { req }),
  runExport: (req: T.ExportRequest, dest: string) => call<T.ExportResult>("run_export", { req, dest }),

  listPeriods: () => call<T.OperationalPeriod[]>("list_periods"),
  currentPeriod: () => call<T.OperationalPeriod | null>("current_period"),
  savePeriod: (id: string | null, input: T.OperationalPeriodInput) =>
    call<T.PeriodSaveResult>("save_period", { id, input }),
  setPeriodVoid: (id: string, isVoid: boolean) =>
    call<T.OperationalPeriod>("set_period_void", { id, void: isVoid }),
  periodResources: (opId: string) => call<T.OpResource[]>("period_resources", { opId }),
  setPeriodResources: (opId: string, rows: T.OpResource[]) =>
    call<T.OpResource[]>("set_period_resources", { opId, rows }),
  form213Preview: (req: T.Form213Request) => call<T.FormPreview>("form213_preview", { req }),
  form214Preview: (req: T.Form214Request) => call<T.FormPreview>("form214_preview", { req }),
  exportForm213: (req: T.Form213Request, dest: string) => call<T.FormResult>("export_form213", { req, dest }),
  exportForm214: (req: T.Form214Request, dest: string) => call<T.FormResult>("export_form214", { req, dest }),

  quitApp: () => call<void>("quit_app"),
  cancelClose: () => call<void>("cancel_close"),
};

export function newId(): string {
  return crypto.randomUUID();
}
