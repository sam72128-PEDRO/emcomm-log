//! Tauri shell: typed commands over `emcomm_core`, native menus, file dialogs (Rust-side, so the
//! webview needs no filesystem capability), backup scheduling, and the close workflow.

#[cfg(target_os = "android")]
mod android;
#[cfg(desktop)]
mod menu;

use emcomm_core::appdb::{AppDb, BackupRun};
use emcomm_core::backup::{self, BackupListing, BackupPreview};
use emcomm_core::backup_health;
use emcomm_core::drafts::{Draft, DraftMeta, DraftStore};
use emcomm_core::error::{AppError, AppResult, ErrorCode};
use emcomm_core::export::{self, ExportPreview, ExportRequest, ExportResult, forms, ics309};
use emcomm_core::fsutil::{self, now_utc};
use emcomm_core::logbook::{CreateParams, Logbook, StorageHealth};
use emcomm_core::models::*;
use emcomm_core::package::{self, ExportPackageResult, ImportResult, PackagePreview};
use emcomm_core::periods;
use emcomm_core::snapshot;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};
#[cfg(not(target_os = "android"))]
use tauri_plugin_dialog::DialogExt;

pub struct AppState {
    app: Mutex<AppDb>,
    book: Mutex<Option<Logbook>>,
    drafts: DraftStore,
    data_dir: PathBuf,
    backup_lock: Mutex<()>,
    backup_running: AtomicBool,
    scheduler_stop: Mutex<Option<Arc<AtomicBool>>>,
    next_auto_due: Mutex<Option<Instant>>,
    close_requested_at: Mutex<Option<Instant>>,
    allow_exit: AtomicBool,
    reveal_allow: Mutex<HashSet<PathBuf>>,
    /// One foreground GPS acquisition at a time; `gps_cancel` stops it and releases the device.
    gps_busy: Arc<AtomicBool>,
    gps_cancel: Arc<AtomicBool>,
    /// 2.2: cancels the attachment copy in progress.
    attach_cancel: Arc<AtomicBool>,
    /// 2.3: last time a scheduled backup was skipped because nothing had changed (shown, never a failure).
    last_unchanged_check: Mutex<Option<String>>,
    /// 2.3: one backup verification at a time.
    verifying: AtomicBool,
}

type Cmd<T> = Result<T, AppError>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn no_book() -> AppError {
    AppError::new(ErrorCode::NoLogbookOpen, "No logbook is open")
}

fn with_book<R>(s: &AppState, f: impl FnOnce(&mut Logbook) -> AppResult<R>) -> AppResult<R> {
    let mut g = lock(&s.book);
    let lb = g.as_mut().ok_or_else(no_book)?;
    f(lb)
}

fn operator_snapshot(s: &AppState) -> AppResult<OperatorSnapshot> {
    let db = lock(&s.app);
    let settings = db.settings()?;
    let pid = settings
        .active_profile_id
        .ok_or_else(|| AppError::validation("profile", "Select an active operator profile in Settings before logging"))?;
    Ok(OperatorSnapshot::from(&db.get_profile(&pid)?))
}

// ---------- views ----------

#[derive(Serialize)]
pub struct Bootstrap {
    settings: GlobalSettings,
    profiles: Vec<Profile>,
    presets: Vec<Preset>,
    app_version: String,
    data_dir: String,
    platform: String,
    active: Option<ActiveView>,
}

#[derive(Serialize, Clone)]
pub struct BackupStatus {
    configured: bool,
    destination: Option<String>,
    interval_minutes: u32,
    running: bool,
    last: Option<BackupRun>,
    last_success: Option<BackupRun>,
    message: Option<String>,
}

#[derive(Serialize)]
pub struct ActiveView {
    info: LogbookInfo,
    settings: LogbookSettings,
    health: StorageHealth,
    notices: Vec<String>,
    drafts: Vec<Draft>,
    backup: BackupStatus,
}

fn backup_status(s: &AppState, logbook_id: &str, live_dir: &Path) -> AppResult<BackupStatus> {
    let db = lock(&s.app);
    let settings = db.settings()?;
    let dest = settings.backup.destination.clone();
    let message = match &dest {
        None => Some("No backup destination is configured. Logging works, but no independent copy is being made.".into()),
        Some(d) => backup::check_destination(Path::new(d), live_dir).err().map(|e| e.message),
    };
    let runs = db.backup_runs(logbook_id, 50)?;
    Ok(BackupStatus {
        configured: dest.is_some(),
        destination: dest,
        interval_minutes: settings.backup.interval_minutes,
        running: s.backup_running.load(Ordering::SeqCst),
        last: runs.first().cloned(),
        last_success: runs.iter().find(|r| r.ok).cloned(),
        message,
    })
}

fn active_view(s: &AppState) -> AppResult<Option<ActiveView>> {
    let (info, settings, health, notices, id, dir) = {
        let g = lock(&s.book);
        let Some(lb) = g.as_ref() else { return Ok(None) };
        (lb.info()?, lb.settings()?, lb.health()?, lb.notices.clone(), lb.id.clone(), lb.dir.clone())
    };
    let drafts = s.drafts.list(&id)?;
    let backup = backup_status(s, &id, &dir)?;
    Ok(Some(ActiveView { info, settings, health, notices, drafts, backup }))
}

fn refresh_index(s: &AppState, lb: &Logbook) -> AppResult<()> {
    let info = lb.info()?;
    let stats = lb.stats(None, None)?;
    let preset = lb.settings()?.preset_id;
    lock(&s.app).register_logbook(&info, &stats, preset.as_deref(), None)
}

fn emit_backup(app: &AppHandle) {
    let s = app.state::<AppState>();
    let ids = lock(&s.book).as_ref().map(|lb| (lb.id.clone(), lb.dir.clone()));
    if let Some((id, dir)) = ids
        && let Ok(st) = backup_status(&s, &id, &dir)
    {
        let _ = app.emit("backup-status", st);
    }
}

// ---------- bootstrap / settings ----------

#[tauri::command]
async fn get_bootstrap(state: State<'_, AppState>) -> Cmd<Bootstrap> {
    let (settings, profiles, presets) = {
        let db = lock(&state.app);
        (db.settings()?, db.list_profiles()?, db.list_presets()?)
    };
    Ok(Bootstrap {
        settings,
        profiles,
        presets,
        app_version: env!("CARGO_PKG_VERSION").into(),
        data_dir: state.data_dir.to_string_lossy().into(),
        platform: std::env::consts::OS.into(),
        active: active_view(&state)?,
    })
}

#[tauri::command]
async fn save_settings(state: State<'_, AppState>, settings: GlobalSettings) -> Cmd<GlobalSettings> {
    lock(&state.app).save_settings(&settings)
}

#[derive(Serialize)]
pub struct FolderCheck {
    ok: bool,
    kind: fsutil::LocationKind,
    message: Option<String>,
}

#[tauri::command]
async fn check_folder(state: State<'_, AppState>, path: String, purpose: String) -> Cmd<FolderCheck> {
    let p = Path::new(&path);
    let kind = fsutil::classify_location(p);
    let mut err = fsutil::check_writable(p).err();
    if err.is_none() && purpose == "live" {
        err = emcomm_core::logbook::check_live_location(p).err();
    }
    if err.is_none() && purpose == "backup" {
        for row in lock(&state.app).list_index()? {
            if fsutil::is_within(p, Path::new(&row.path)) {
                err = Some(AppError::validation("path", format!("This folder is inside the live logbook '{}'", row.name)));
            }
        }
    }
    let warn = if err.is_none() && purpose == "backup" && kind != fsutil::LocationKind::Local {
        Some("This is a network or cloud-synced folder. Backups will work, but keep a local copy as well.".to_string())
    } else {
        None
    };
    Ok(FolderCheck { ok: err.is_none(), kind, message: err.map(|e| e.message).or(warn) })
}

#[derive(Deserialize)]
pub struct DialogFilter {
    #[cfg_attr(target_os = "android", allow(dead_code))]
    name: String,
    extensions: Vec<String>,
}

#[cfg(not(target_os = "android"))]
fn file_dialog(
    app: &AppHandle,
    title: &str,
    filters: &[DialogFilter],
    start: Option<&str>,
) -> tauri_plugin_dialog::FileDialogBuilder<tauri::Wry> {
    let mut d = app.dialog().file().set_title(title);
    for f in filters {
        let exts: Vec<&str> = f.extensions.iter().map(String::as_str).collect();
        d = d.add_filter(&f.name, &exts);
    }
    if let Some(dir) = start.filter(|d| Path::new(d).is_dir()) {
        d = d.set_directory(dir);
    }
    #[cfg(desktop)]
    if let Some(w) = app.get_webview_window("main") {
        d = d.set_parent(&w);
    }
    d
}

/// Android spike: there is no folder picker that yields a real filesystem path (SAF gives
/// `content://` tree URIs, and SQLite needs a path), so "choose a folder" resolves to a fixed
/// folder in app-private storage. Backups and exports get their own app-private folders.
#[cfg(mobile)]
fn mobile_private_dir(app: &AppHandle, kind: &str) -> AppResult<PathBuf> {
    let base = app.state::<AppState>().data_dir.clone();
    let dir = base.join(kind);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(not(target_os = "android"))]
fn to_path(p: tauri_plugin_dialog::FilePath) -> Option<String> {
    p.into_path().ok().map(|p| p.to_string_lossy().to_string())
}

#[cfg(desktop)]
#[tauri::command]
async fn pick_folder(app: AppHandle, title: String, start: Option<String>) -> Cmd<Option<String>> {
    Ok(file_dialog(&app, &title, &[], start.as_deref()).blocking_pick_folder().and_then(to_path))
}

#[cfg(mobile)]
#[tauri::command]
async fn pick_folder(app: AppHandle, title: String, start: Option<String>) -> Cmd<Option<String>> {
    let _ = start;
    let kind = if title.to_lowercase().contains("backup") { "backups" } else { "logbooks" };
    Ok(Some(mobile_private_dir(&app, kind)?.to_string_lossy().to_string()))
}

#[cfg(not(target_os = "android"))]
#[tauri::command]
async fn pick_file(app: AppHandle, title: String, filters: Vec<DialogFilter>) -> Cmd<Option<String>> {
    Ok(file_dialog(&app, &title, &filters, None).blocking_pick_file().and_then(to_path))
}

/// Android: the system picker returns a `content://` document; the bridge copies it into the
/// app cache so the core can validate and read a real file.
#[cfg(target_os = "android")]
#[tauri::command]
async fn pick_file(app: AppHandle, title: String, filters: Vec<DialogFilter>) -> Cmd<Option<String>> {
    let _ = title;
    let exts: Vec<String> = filters.into_iter().flat_map(|f| f.extensions).collect();
    android::pick_document(&app, &exts)
}

#[tauri::command]
async fn pick_save_file(
    app: AppHandle,
    title: String,
    default_name: String,
    filters: Vec<DialogFilter>,
    start: Option<String>,
) -> Cmd<Option<String>> {
    let safe = fsutil::safe_component(&default_name, 120);
    #[cfg(target_os = "android")]
    {
        // Android "Save to…" (system document picker). The export is written to the app cache
        // and copied to the chosen document when it is complete (see `delivered`).
        let _ = (title, filters, start);
        android::choose_save_target(&app, &safe)
    }
    #[cfg(desktop)]
    Ok(file_dialog(&app, &title, &filters, start.as_deref()).set_file_name(safe).blocking_save_file().and_then(to_path))
}

// ---------- profiles & presets ----------

#[tauri::command]
async fn create_profile(state: State<'_, AppState>, input: ProfileInput) -> Cmd<Profile> {
    lock(&state.app).create_profile(&input)
}
#[tauri::command]
async fn update_profile(state: State<'_, AppState>, id: String, input: ProfileInput) -> Cmd<Profile> {
    lock(&state.app).update_profile(&id, &input)
}
#[tauri::command]
async fn set_profile_active(state: State<'_, AppState>, id: String, active: bool) -> Cmd<Profile> {
    lock(&state.app).set_profile_active(&id, active)
}
#[tauri::command]
async fn list_profiles(state: State<'_, AppState>) -> Cmd<Vec<Profile>> {
    lock(&state.app).list_profiles()
}
#[tauri::command]
async fn list_presets(state: State<'_, AppState>) -> Cmd<Vec<Preset>> {
    lock(&state.app).list_presets()
}
#[tauri::command]
async fn create_preset(state: State<'_, AppState>, input: PresetInput) -> Cmd<Preset> {
    lock(&state.app).create_preset(&input)
}
#[tauri::command]
async fn update_preset(state: State<'_, AppState>, id: String, input: PresetInput) -> Cmd<Preset> {
    lock(&state.app).update_preset(&id, &input)
}
#[tauri::command]
async fn duplicate_preset(state: State<'_, AppState>, id: String) -> Cmd<Preset> {
    lock(&state.app).duplicate_preset(&id)
}
#[tauri::command]
async fn delete_preset(state: State<'_, AppState>, id: String) -> Cmd<()> {
    lock(&state.app).delete_preset(&id)
}

// ---------- logbooks ----------

#[tauri::command]
async fn list_logbooks(state: State<'_, AppState>) -> Cmd<Vec<LogbookIndexRow>> {
    // The open logbook's counts change with every save; refresh its row so the list is current.
    if let Some(lb) = lock(&state.book).as_ref() {
        let _ = refresh_index(&state, lb);
    }
    lock(&state.app).list_index()
}

fn start_scheduler(app: &AppHandle) {
    let state = app.state::<AppState>();
    let stop = Arc::new(AtomicBool::new(false));
    if let Some(old) = lock(&state.scheduler_stop).replace(stop.clone()) {
        old.store(true, Ordering::SeqCst);
    }
    *lock(&state.next_auto_due) = Some(Instant::now() + Duration::from_secs(60));
    let app = app.clone();
    std::thread::spawn(move || {
        loop {
            for _ in 0..10 {
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
            let state = app.state::<AppState>();
            let due = lock(&state.next_auto_due).map(|d| Instant::now() >= d).unwrap_or(false);
            if !due {
                continue;
            }
            let interval = lock(&state.app).settings().map(|s| s.backup.interval_minutes).unwrap_or(15);
            *lock(&state.next_auto_due) = Some(Instant::now() + Duration::from_secs(interval as u64 * 60));
            let _ = run_backup_kind(&app, "auto", true);
        }
    });
}

fn stop_scheduler(s: &AppState) {
    if let Some(stop) = lock(&s.scheduler_stop).take() {
        stop.store(true, Ordering::SeqCst);
    }
}

#[derive(Serialize, Clone)]
pub struct BackupOutcome {
    ok: bool,
    skipped: bool,
    path: Option<String>,
    error: Option<AppError>,
}

/// Run one backup of the active logbook without holding the logbook lock (the snapshot uses
/// its own read connection), record the outcome, and notify the UI.
fn run_backup_kind(app: &AppHandle, kind: &str, skip_if_unchanged: bool) -> BackupOutcome {
    let state = app.state::<AppState>();
    let Some((id, dir)) = lock(&state.book).as_ref().map(|lb| (lb.id.clone(), lb.dir.clone())) else {
        return BackupOutcome { ok: false, skipped: true, path: None, error: Some(no_book()) };
    };
    let settings = match lock(&state.app).settings() {
        Ok(s) => s,
        Err(e) => return BackupOutcome { ok: false, skipped: false, path: None, error: Some(e) },
    };
    let Some(dest) = settings.backup.destination.clone() else {
        return BackupOutcome {
            ok: false,
            skipped: true,
            path: None,
            error: Some(AppError::new(ErrorCode::Validation, "No backup destination is configured")),
        };
    };
    let _guard = lock(&state.backup_lock);
    if skip_if_unchanged {
        let last = lock(&state.app).last_successful_backup(&id).ok().flatten();
        if let (Some(last), Ok(marker)) = (last, snapshot::live_data_marker(&dir))
            && last.data_marker.as_deref() == Some(marker.as_str())
        {
            *lock(&state.last_unchanged_check) = Some(now_utc());
            return BackupOutcome { ok: true, skipped: true, path: last.path, error: None };
        }
    }
    state.backup_running.store(true, Ordering::SeqCst);
    emit_backup(app);
    let started = now_utc();
    let result = backup::run_backup(&dir, Path::new(&dest), kind, &settings.backup);
    state.backup_running.store(false, Ordering::SeqCst);
    let run = BackupRun {
        logbook_id: id.clone(),
        kind: kind.into(),
        started_at_utc: started,
        finished_at_utc: now_utc(),
        ok: result.is_ok(),
        path: result.as_ref().ok().map(|r| r.path.clone()),
        entry_count: result.as_ref().ok().map(|r| r.entry_count),
        message: match &result {
            Ok(r) if r.pruned > 0 => format!("Created and checked; removed {} old backup(s) per retention", r.pruned),
            Ok(_) => "Created and checked".into(),
            Err(e) => e.message.clone(),
        },
        data_marker: result.as_ref().ok().map(|r| r.data_marker.clone()),
    };
    let _ = lock(&state.app).record_backup(&run);
    emit_backup(app);
    match result {
        Ok(r) => BackupOutcome { ok: true, skipped: false, path: Some(r.path), error: None },
        Err(e) => BackupOutcome { ok: false, skipped: false, path: None, error: Some(e) },
    }
}

fn activate(app: &AppHandle, lb: Logbook) -> AppResult<ActiveView> {
    let state = app.state::<AppState>();
    let keep = state.drafts.referenced_staging(&lb.id);
    let _ = lb.cleanup_orphans(&keep);
    refresh_index(&state, &lb)?;
    lock(&state.app).touch_opened(&lb.id)?;
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_title(&format!("EMCOMM LOG - {}", lb.info()?.name));
    }
    *lock(&state.book) = Some(lb);
    start_scheduler(app);
    active_view(&state)?.ok_or_else(no_book)
}

fn ensure_none_open(s: &AppState) -> AppResult<()> {
    if lock(&s.book).is_some() {
        return Err(AppError::new(ErrorCode::Conflict, "Close the current logbook first"));
    }
    Ok(())
}

#[tauri::command]
async fn create_logbook(app: AppHandle, state: State<'_, AppState>, req: CreateLogbookRequest) -> Cmd<ActiveView> {
    ensure_none_open(&state)?;
    let (profile, preset) = {
        let db = lock(&state.app);
        let profile = db.get_profile(&req.profile_id)?;
        let preset = match &req.preset_id {
            Some(id) => {
                let p = db.get_preset(id)?;
                if p.logbook_type != req.logbook_type {
                    return Err(AppError::validation("preset_id", "Preset is for a different logbook type"));
                }
                Some(p)
            }
            None => None,
        };
        (profile, preset)
    };
    let dir = Logbook::create(CreateParams {
        logbook_type: req.logbook_type,
        name: req.name.clone(),
        parent_dir: PathBuf::from(&req.parent_dir),
        incident: req.incident.clone(),
        profile: OperatorSnapshot::from(&profile),
        preset,
    })?;
    let lb = Logbook::open(&dir)?;
    activate(&app, lb)
}

#[tauri::command]
async fn open_logbook(app: AppHandle, state: State<'_, AppState>, id: String) -> Cmd<ActiveView> {
    ensure_none_open(&state)?;
    let row = lock(&state.app).list_index()?.into_iter().find(|r| r.id == id).ok_or_else(|| AppError::not_found("Logbook not in list"))?;
    let lb = Logbook::open(Path::new(&row.path))?;
    if lb.id != row.id {
        return Err(AppError::new(
            ErrorCode::Conflict,
            "The folder at this path now holds a different logbook. Use Relink to choose the right folder.",
        ));
    }
    activate(&app, lb)
}

/// Open (and register) an existing live logbook folder that is not yet in the local list.
#[tauri::command]
async fn open_logbook_folder(app: AppHandle, state: State<'_, AppState>, path: String) -> Cmd<ActiveView> {
    ensure_none_open(&state)?;
    let lb = Logbook::open(Path::new(&path))?;
    activate(&app, lb)
}

#[derive(Serialize)]
pub struct CloseResult {
    closed: bool,
    backup: Option<BackupOutcome>,
}

#[tauri::command]
async fn close_logbook(app: AppHandle, state: State<'_, AppState>, backup: bool) -> Cmd<CloseResult> {
    if lock(&state.book).is_none() {
        return Ok(CloseResult { closed: true, backup: None });
    }
    let on_close = lock(&state.app).settings()?.backup.on_close;
    let mut outcome = None;
    if backup && on_close {
        let o = run_backup_kind(&app, "close", false);
        if !o.ok && !o.skipped {
            return Ok(CloseResult { closed: false, backup: Some(o) });
        }
        outcome = Some(o);
    }
    stop_scheduler(&state);
    let lb = lock(&state.book).take();
    if let Some(lb) = lb {
        let _ = refresh_index(&state, &lb);
        lb.close()?;
    }
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_title("EMCOMM LOG");
    }
    Ok(CloseResult { closed: true, backup: outcome })
}

#[tauri::command]
async fn relink_logbook(state: State<'_, AppState>, id: String, path: String) -> Cmd<Vec<LogbookIndexRow>> {
    let d = emcomm_core::logbook::read_descriptor(Path::new(&path))
        .ok_or_else(|| AppError::validation("path", "That folder is not an EMCOMM LOG logbook"))?;
    if d.logbook_id != id {
        return Err(AppError::validation("path", format!("That folder holds a different logbook ('{}')", d.name)));
    }
    let mut db = lock(&state.app);
    db.relink(&id, Path::new(&path))?;
    db.list_index()
}

#[tauri::command]
async fn forget_logbook(state: State<'_, AppState>, id: String) -> Cmd<Vec<LogbookIndexRow>> {
    if lock(&state.book).as_ref().is_some_and(|lb| lb.id == id) {
        return Err(AppError::new(ErrorCode::Conflict, "Close the logbook before removing it from the list"));
    }
    let mut db = lock(&state.app);
    db.forget_logbook(&id)?;
    db.list_index()
}

/// Apply a change to a logbook that may or may not be the active one.
fn with_any_book<R>(state: &AppState, id: &str, f: impl FnOnce(&mut Logbook) -> AppResult<R>) -> AppResult<R> {
    {
        let mut g = lock(&state.book);
        if let Some(lb) = g.as_mut().filter(|lb| lb.id == id) {
            let r = f(lb)?;
            refresh_index(state, lb)?;
            return Ok(r);
        }
    }
    let row = lock(&state.app).list_index()?.into_iter().find(|r| r.id == id).ok_or_else(|| AppError::not_found("Logbook not in list"))?;
    let mut lb = Logbook::open(Path::new(&row.path))?;
    let r = f(&mut lb)?;
    refresh_index(state, &lb)?;
    lb.close()?;
    Ok(r)
}

#[tauri::command]
async fn set_logbook_archived(state: State<'_, AppState>, id: String, archived: bool) -> Cmd<Vec<LogbookIndexRow>> {
    with_any_book(&state, &id, |lb| lb.set_archived(archived))?;
    lock(&state.app).list_index()
}

#[tauri::command]
async fn rename_logbook(state: State<'_, AppState>, id: String, name: String) -> Cmd<Vec<LogbookIndexRow>> {
    with_any_book(&state, &id, |lb| {
        let inc = lb.info()?.incident;
        lb.update_identity(&name, &inc).map(|_| ())
    })?;
    lock(&state.app).list_index()
}

#[tauri::command]
async fn update_logbook_identity(app: AppHandle, state: State<'_, AppState>, name: String, incident: IncidentFields) -> Cmd<LogbookInfo> {
    let info = with_book(&state, |lb| lb.update_identity(&name, &incident))?;
    with_book(&state, |lb| refresh_index(&state, lb))?;
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_title(&format!("EMCOMM LOG - {}", info.name));
    }
    Ok(info)
}

#[tauri::command]
async fn update_logbook_settings(
    state: State<'_, AppState>,
    time_display_override: Option<String>,
    operational_notes: String,
) -> Cmd<LogbookSettings> {
    with_book(&state, |lb| lb.update_settings(time_display_override, operational_notes))
}

#[tauri::command]
async fn get_active(state: State<'_, AppState>) -> Cmd<Option<ActiveView>> {
    active_view(&state)
}

#[tauri::command]
async fn logbook_stats(state: State<'_, AppState>, day_start_utc: Option<String>, day_end_utc: Option<String>) -> Cmd<LogbookStats> {
    with_book(&state, |lb| lb.stats(day_start_utc.as_deref(), day_end_utc.as_deref()))
}

#[tauri::command]
async fn storage_health(state: State<'_, AppState>) -> Cmd<StorageHealth> {
    with_book(&state, |lb| lb.health())
}

fn allowed_reveal_roots(state: &AppState) -> AppResult<Vec<PathBuf>> {
    let mut roots: Vec<PathBuf> = lock(&state.app).list_index()?.into_iter().map(|r| PathBuf::from(r.path)).collect();
    if let Some(d) = lock(&state.app).settings()?.backup.destination {
        roots.push(PathBuf::from(d));
    }
    roots.extend(lock(&state.reveal_allow).iter().cloned());
    roots.push(state.data_dir.clone());
    Ok(roots)
}

/// Android: "open" hands the file to another app; "reveal" opens the share sheet.
#[cfg(target_os = "android")]
fn os_open(app: &AppHandle, path: &Path, reveal: bool) -> AppResult<()> {
    if reveal { android::share_file(app, path) } else { android::view_file(app, path) }
}

#[cfg(not(target_os = "android"))]
fn os_open(_app: &AppHandle, path: &Path, reveal: bool) -> AppResult<()> {
    use std::process::Command;
    let status = if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        if reveal {
            c.arg("-R");
        }
        c.arg(path).status()
    } else if cfg!(windows) {
        if reveal {
            Command::new("explorer").arg(format!("/select,{}", path.display())).status()
        } else {
            Command::new("explorer").arg(path).status()
        }
    } else {
        let target = if reveal { path.parent().unwrap_or(path) } else { path };
        Command::new("xdg-open").arg(target).status()
    };
    status.map(|_| ()).map_err(|e| AppError::new(ErrorCode::Io, format!("Could not open the file manager: {e}")))
}

#[tauri::command]
async fn reveal_path(app: AppHandle, state: State<'_, AppState>, path: String) -> Cmd<()> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(AppError::new(ErrorCode::StorageUnavailable, "That location is not available"));
    }
    if !allowed_reveal_roots(&state)?.iter().any(|r| fsutil::is_within(&p, r)) {
        return Err(AppError::new(ErrorCode::PermissionDenied, "Only logbook, backup, and export locations can be revealed"));
    }
    os_open(&app, &p, true)
}

/// Open a file this session exported (e.g. a PDF for printing) in the default application.
#[tauri::command]
async fn open_export(app: AppHandle, state: State<'_, AppState>, path: String) -> Cmd<()> {
    let p = PathBuf::from(&path);
    if !lock(&state.reveal_allow).contains(&p) || !p.is_file() {
        return Err(AppError::new(ErrorCode::PermissionDenied, "Only files exported in this session can be opened"));
    }
    os_open(&app, &p, false)
}

#[tauri::command]
async fn open_attachment(app: AppHandle, state: State<'_, AppState>, attachment_id: String) -> Cmd<()> {
    let p = with_book(&state, |lb| lb.attachment_path(&attachment_id))?;
    let (name, _) = with_book(&state, |lb| lb.attachment_meta(&attachment_id))?;
    // 2.2 ATT-06: never hand a program or script to the OS to run, whatever it is called.
    if emcomm_core::attach::is_risky(&name, emcomm_core::attach::sniff_file(&p)?) {
        return Err(AppError::validation(
            "attachments",
            "EMCOMM LOG does not open programs or scripts. Use Save As… to copy it somewhere if you need it.",
        ));
    }
    os_open(&app, &p, false)
}

// ---------- entries ----------

#[tauri::command]
async fn save_entry(state: State<'_, AppState>, input: EntryInput, request_id: String, draft_id: Option<String>) -> Cmd<SaveResult> {
    let op = operator_snapshot(&state)?;
    let (res, lbid) = with_book(&state, |lb| {
        let who = lb.ensure_operator(&op)?;
        Ok((lb.save_new(&input, &request_id, &who)?, lb.id.clone()))
    })?;
    if let Some(d) = draft_id {
        let _ = state.drafts.delete(&lbid, &d);
    }
    Ok(res)
}

#[tauri::command]
async fn save_changes(
    state: State<'_, AppState>,
    entry_id: String,
    base_revision: i64,
    input: EntryInput,
    reason: String,
    request_id: String,
    draft_id: Option<String>,
) -> Cmd<SaveResult> {
    let op = operator_snapshot(&state)?;
    let (res, lbid) = with_book(&state, |lb| {
        let who = lb.ensure_operator(&op)?;
        Ok((lb.save_changes(&entry_id, base_revision, &input, &reason, &request_id, &who)?, lb.id.clone()))
    })?;
    if let Some(d) = draft_id {
        let _ = state.drafts.delete(&lbid, &d);
    }
    Ok(res)
}

#[tauri::command]
async fn void_entry(state: State<'_, AppState>, entry_id: String, reason: String, request_id: String) -> Cmd<SaveResult> {
    let op = operator_snapshot(&state)?;
    with_book(&state, |lb| {
        let who = lb.ensure_operator(&op)?;
        lb.void_entry(&entry_id, &reason, &request_id, &who)
    })
}

#[tauri::command]
async fn restore_entry(state: State<'_, AppState>, entry_id: String, reason: String, request_id: String) -> Cmd<SaveResult> {
    let op = operator_snapshot(&state)?;
    with_book(&state, |lb| {
        let who = lb.ensure_operator(&op)?;
        lb.restore_entry(&entry_id, &reason, &request_id, &who)
    })
}

#[tauri::command]
async fn get_entry(state: State<'_, AppState>, id: String) -> Cmd<Entry> {
    with_book(&state, |lb| lb.get_entry(&id))
}

#[tauri::command]
async fn list_entries(state: State<'_, AppState>, query: EntryQuery) -> Cmd<EntryPage> {
    with_book(&state, |lb| lb.list_entries(&query))
}

#[tauri::command]
async fn entry_revisions(state: State<'_, AppState>, id: String) -> Cmd<Vec<EntryRevision>> {
    with_book(&state, |lb| lb.revisions(&id))
}

#[tauri::command]
async fn set_favorite(state: State<'_, AppState>, id: String, favorite: bool) -> Cmd<Entry> {
    with_book(&state, |lb| lb.set_favorite(&id, favorite))
}

#[tauri::command]
async fn stage_attachment(app: AppHandle, state: State<'_, AppState>) -> Cmd<Option<StagedAttachment>> {
    with_book(&state, |lb| lb.ensure_writable())?;
    #[cfg(not(target_os = "android"))]
    let picked = file_dialog(&app, "Attach a file", &[], None).blocking_pick_file().and_then(to_path);
    #[cfg(target_os = "android")]
    let picked = android::pick_document(&app, &[])?;
    let Some(path) = picked else { return Ok(None) };
    with_book(&state, |lb| lb.stage_attachment(Path::new(&path))).map(Some)
}

/// 2.2: choose one or more files to attach (native dialog). Returns paths with sizes so the UI can warn
/// about large files before anything is copied.
#[derive(Serialize)]
struct PickedFile {
    path: String,
    name: String,
    size_bytes: u64,
}

fn picked(paths: Vec<String>) -> Vec<PickedFile> {
    paths
        .into_iter()
        .filter_map(|p| {
            let m = std::fs::metadata(&p).ok()?;
            m.is_file().then(|| PickedFile {
                name: Path::new(&p).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                size_bytes: m.len(),
                path: p,
            })
        })
        .collect()
}

#[tauri::command]
async fn pick_attachments(app: AppHandle, state: State<'_, AppState>) -> Cmd<Vec<PickedFile>> {
    with_book(&state, |lb| lb.ensure_writable())?;
    #[cfg(not(target_os = "android"))]
    let paths: Vec<String> =
        file_dialog(&app, "Attach files", &[], None).blocking_pick_files().unwrap_or_default().into_iter().filter_map(to_path).collect();
    #[cfg(target_os = "android")]
    let paths = android::pick_documents(&app)?;
    Ok(picked(paths))
}

/// Android: one photo from the camera app (staged by the UI with source "camera"). Empty elsewhere.
#[tauri::command]
async fn take_photo(app: AppHandle, state: State<'_, AppState>) -> Cmd<Vec<PickedFile>> {
    with_book(&state, |lb| lb.ensure_writable())?;
    #[cfg(target_os = "android")]
    return Ok(picked(android::take_photo(&app)?.into_iter().collect()));
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Err(AppError::validation("attachments", "Taking a photo is available on the tablet only"))
    }
}

/// Sizes of dropped files (drag and drop gives paths only).
#[tauri::command]
async fn describe_files(paths: Vec<String>) -> Cmd<Vec<PickedFile>> {
    Ok(picked(paths))
}

#[derive(Clone, Serialize)]
struct AttachProgress {
    token: String,
    done: u64,
    total: u64,
}

/// Copy one file into staging (streamed, hashed). Progress is emitted as `attach-progress`.
#[tauri::command]
async fn stage_attachment_path(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    source: String,
    token: String,
) -> Cmd<StagedAttachment> {
    state.attach_cancel.store(false, Ordering::SeqCst);
    let cancel = state.attach_cancel.clone();
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let st = app2.state::<AppState>();
        with_book(&st, |lb| {
            lb.stage_attachment_from(Path::new(&path), &source, Some(&cancel), &mut |done, total| {
                let _ = app2.emit("attach-progress", AttachProgress { token: token.clone(), done, total });
            })
        })
    })
    .await
    .map_err(|e| AppError::new(ErrorCode::Io, format!("Attachment copy stopped: {e}")))?
}

#[tauri::command]
async fn cancel_attach(state: State<'_, AppState>) -> Cmd<()> {
    state.attach_cancel.store(true, Ordering::SeqCst);
    Ok(())
}

/// Undo `encodeURIComponent` (header values are ASCII); invalid sequences are kept as typed.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A pasted image: raw bytes in the request body (never base64 in JSON); name and source in headers.
#[tauri::command]
async fn stage_attachment_bytes(state: State<'_, AppState>, request: tauri::ipc::Request<'_>) -> Cmd<StagedAttachment> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(AppError::validation("attachments", "Expected the image bytes"));
    };
    let h = |k: &str| request.headers().get(k).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let (name, source) = (percent_decode(&h("x-name")), h("x-source"));
    with_book(&state, |lb| lb.stage_attachment_bytes(if name.is_empty() { "pasted image.png" } else { &name }, bytes, &source))
}

/// A small PNG thumbnail (data URL) of a saved or staged image; None for other files.
#[tauri::command]
async fn attachment_thumbnail(
    state: State<'_, AppState>,
    attachment_id: Option<String>,
    staging_id: Option<String>,
) -> Cmd<Option<String>> {
    use base64::Engine;
    let path = with_book(&state, |lb| match (&attachment_id, &staging_id) {
        (Some(id), _) => lb.attachment_path(id),
        (None, Some(sid)) => lb.staged_path(sid),
        _ => Err(AppError::validation("attachments", "No attachment given")),
    })?;
    let png = tauri::async_runtime::spawn_blocking(move || emcomm_core::attach::thumbnail_png(&path))
        .await
        .map_err(|e| AppError::new(ErrorCode::Io, e.to_string()))??;
    Ok(png.map(|b| format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(b))))
}

#[tauri::command]
async fn save_attachment_as(app: AppHandle, state: State<'_, AppState>, attachment_id: String) -> Cmd<Option<String>> {
    let (name, _) = with_book(&state, |lb| lb.attachment_meta(&attachment_id))?;
    let safe = fsutil::safe_component(&name, 120);
    #[cfg(not(target_os = "android"))]
    let dest = file_dialog(&app, "Save attachment as", &[], None).set_file_name(safe).blocking_save_file().and_then(to_path);
    // Android: "Save to…" — written to the cache, then copied to the chosen document.
    #[cfg(target_os = "android")]
    let dest = android::choose_save_target(&app, &safe)?;
    let Some(dest) = dest else {
        return Ok(None);
    };
    let r = with_book(&state, |lb| lb.save_attachment_as(&attachment_id, Path::new(&dest)));
    delivered(&app, &dest, r)?;
    Ok(Some(dest))
}

#[tauri::command]
async fn discard_staged(state: State<'_, AppState>, staging_id: String) -> Cmd<()> {
    with_book(&state, |lb| {
        lb.discard_staged(&staging_id);
        Ok(())
    })
}

#[tauri::command]
async fn staged_exists(state: State<'_, AppState>, staging_ids: Vec<String>) -> Cmd<Vec<String>> {
    with_book(&state, |lb| Ok(staging_ids.into_iter().filter(|id| lb.staged_exists(id)).collect()))
}

#[tauri::command]
async fn list_templates(state: State<'_, AppState>) -> Cmd<Vec<EntryTemplate>> {
    with_book(&state, |lb| lb.templates())
}

#[tauri::command]
async fn save_template(state: State<'_, AppState>, id: Option<String>, name: String, defaults: TemplateDefaults) -> Cmd<EntryTemplate> {
    with_book(&state, |lb| lb.save_template(id.as_deref(), &name, &defaults))
}

#[tauri::command]
async fn delete_template(state: State<'_, AppState>, id: String) -> Cmd<()> {
    with_book(&state, |lb| lb.delete_template(&id))
}

// ---------- drafts ----------

#[tauri::command]
async fn save_draft(state: State<'_, AppState>, meta: DraftMeta, content: serde_json::Value) -> Cmd<String> {
    state.drafts.save(&meta, &content)
}

#[tauri::command]
async fn list_drafts(state: State<'_, AppState>) -> Cmd<Vec<Draft>> {
    let id = with_book(&state, |lb| Ok(lb.id.clone()))?;
    state.drafts.list(&id)
}

#[tauri::command]
async fn delete_draft(state: State<'_, AppState>, logbook_id: String, draft_id: String) -> Cmd<()> {
    state.drafts.delete(&logbook_id, &draft_id)
}

// ---------- backups ----------

#[tauri::command]
async fn run_backup_now(app: AppHandle) -> Cmd<BackupOutcome> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || run_backup_kind(&app2, "manual", false))
        .await
        .map_err(|e| AppError::internal(e.to_string()))
}

#[tauri::command]
async fn get_backup_status(state: State<'_, AppState>) -> Cmd<BackupStatus> {
    let (id, dir) = with_book(&state, |lb| Ok((lb.id.clone(), lb.dir.clone())))?;
    backup_status(&state, &id, &dir)
}

#[tauri::command]
async fn list_backups(state: State<'_, AppState>, destination: Option<String>) -> Cmd<Vec<BackupListing>> {
    let dest = match destination {
        Some(d) => d,
        None => lock(&state.app)
            .settings()?
            .backup
            .destination
            .ok_or_else(|| AppError::validation("destination", "No backup destination is configured"))?,
    };
    lock(&state.reveal_allow).insert(PathBuf::from(&dest));
    tauri::async_runtime::spawn_blocking(move || backup::list_backups(Path::new(&dest)))
        .await
        .map_err(|e| AppError::internal(e.to_string()))?
}

#[tauri::command]
async fn preview_backup(path: String) -> Cmd<BackupPreview> {
    tauri::async_runtime::spawn_blocking(move || backup::preview_backup(Path::new(&path)))
        .await
        .map_err(|e| AppError::internal(e.to_string()))?
}

#[tauri::command]
async fn restore_backup(state: State<'_, AppState>, path: String, parent_dir: String, name: Option<String>) -> Cmd<LogbookIndexRow> {
    let known: HashSet<String> = lock(&state.app).list_index()?.into_iter().map(|r| r.id).collect();
    let out = tauri::async_runtime::spawn_blocking(move || {
        backup::restore_backup(Path::new(&path), Path::new(&parent_dir), name.as_deref(), &|id| known.contains(id))
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))??;
    // Verify by opening the restored book before reporting success.
    let lb = Logbook::open(&out.path)?;
    let info = lb.info()?;
    let stats = lb.stats(None, None)?;
    {
        let mut db = lock(&state.app);
        db.register_logbook(&info, &stats, None, Some(&out.source_uuid))?;
    }
    lb.close()?;
    lock(&state.app)
        .list_index()?
        .into_iter()
        .find(|r| r.id == info.id)
        .ok_or_else(|| AppError::internal("restored logbook not registered"))
}

/// Android: copy a finished export to the document chosen with "Save to…". No-op elsewhere.
#[cfg(target_os = "android")]
fn delivered<T>(app: &AppHandle, dest: &str, r: AppResult<T>) -> AppResult<T> {
    match r {
        Ok(v) => android::deliver_export(app, dest).map(|_| v),
        Err(e) => {
            android::forget_export(dest);
            Err(e)
        }
    }
}

#[cfg(not(target_os = "android"))]
fn delivered<T>(_app: &AppHandle, _dest: &str, r: AppResult<T>) -> AppResult<T> {
    r
}

// ---------- packages ----------

/// Export the open logbook, or any listed logbook by id (read through a consistent snapshot).
#[tauri::command]
async fn export_package(app: AppHandle, state: State<'_, AppState>, id: Option<String>, dest: String) -> Cmd<ExportPackageResult> {
    let active = lock(&state.book).as_ref().map(|lb| (lb.id.clone(), lb.dir.clone()));
    let dir = match (id, active) {
        (None, Some((_, d))) => d,
        (Some(id), Some((aid, d))) if id == aid => d,
        (Some(id), _) => PathBuf::from(
            lock(&state.app).list_index()?.into_iter().find(|r| r.id == id).ok_or_else(|| AppError::not_found("Logbook not in list"))?.path,
        ),
        (None, None) => return Err(no_book()),
    };
    let d = dest.clone();
    let r = tauri::async_runtime::spawn_blocking(move || package::export_package(&dir, Path::new(&d)))
        .await
        .map_err(|e| AppError::internal(e.to_string()))
        .and_then(|r| r);
    let r = delivered(&app, &dest, r)?;
    lock(&state.reveal_allow).insert(PathBuf::from(dest));
    Ok(r)
}

#[tauri::command]
async fn preview_package(path: String) -> Cmd<PackagePreview> {
    tauri::async_runtime::spawn_blocking(move || package::preview_package(Path::new(&path)))
        .await
        .map_err(|e| AppError::internal(e.to_string()))?
}

#[derive(Serialize)]
pub struct ImportOutcome {
    result: ImportResult,
    logbook: LogbookIndexRow,
}

#[tauri::command]
async fn import_package(state: State<'_, AppState>, path: String, parent_dir: String, name: Option<String>) -> Cmd<ImportOutcome> {
    let known: Vec<(String, Option<String>, String)> =
        lock(&state.app).list_index()?.into_iter().map(|r| (r.id, r.source_uuid, r.name)).collect();
    let result = tauri::async_runtime::spawn_blocking(move || {
        package::import_package(Path::new(&path), Path::new(&parent_dir), name.as_deref(), &known)
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))??;
    let lb = Logbook::open(Path::new(&result.path))?;
    let info = lb.info()?;
    let stats = lb.stats(None, None)?;
    lock(&state.app).register_logbook(&info, &stats, None, Some(&result.source_uuid))?;
    lb.close()?;
    let logbook = lock(&state.app)
        .list_index()?
        .into_iter()
        .find(|r| r.id == info.id)
        .ok_or_else(|| AppError::internal("imported logbook not registered"))?;
    Ok(ImportOutcome { result, logbook })
}

// ---------- report exports ----------

#[tauri::command]
async fn export_preview(state: State<'_, AppState>, req: ExportRequest) -> Cmd<ExportPreview> {
    with_book(&state, |lb| export::preview(&lb.conn, lb.logbook_type, &req))
}

/// 2.2 Review Export: every entry in the scope, eligible or not (with the reason). Changes nothing.
#[tauri::command]
async fn export_review(state: State<'_, AppState>, req: ExportRequest) -> Cmd<export::review::ExportReview> {
    with_book(&state, |lb| export::review(&lb.conn, lb.logbook_type, &req))
}

#[tauri::command]
async fn form_review(state: State<'_, AppState>, form: String, req: serde_json::Value) -> Cmd<export::review::ExportReview> {
    let bad = |e: serde_json::Error| AppError::validation("form", format!("Invalid form request: {e}"));
    with_book(&state, |lb| {
        let info = lb.info()?;
        match form.as_str() {
            "213" => forms::review_213(&lb.conn, &info, &serde_json::from_value(req).map_err(bad)?),
            "214" => forms::review_214(&lb.conn, &info, &serde_json::from_value(req).map_err(bad)?),
            "309" => ics309::review_309(&lb.conn, &info, &serde_json::from_value(req).map_err(bad)?),
            _ => Err(AppError::validation("form", "Unknown form")),
        }
    })
}

#[tauri::command]
async fn run_export(app: AppHandle, state: State<'_, AppState>, req: ExportRequest, dest: String) -> Cmd<ExportResult> {
    let (db_path, info) = with_book(&state, |lb| Ok((lb.db_path(), lb.info()?)))?;
    let d = dest.clone();
    let r = tauri::async_runtime::spawn_blocking(move || {
        let conn = Logbook::open_read_only(&db_path)?;
        export::run_export(&conn, &info, &req, Path::new(&d))
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))
    .and_then(|r| r);
    let r = delivered(&app, &dest, r)?;
    lock(&state.reveal_allow).insert(PathBuf::from(dest));
    Ok(r)
}

// ---------- station and operator shifts (2.1) ----------

#[tauri::command]
async fn get_station(state: State<'_, AppState>) -> Cmd<Station> {
    with_book(&state, |lb| lb.station())
}

#[tauri::command]
async fn set_station(state: State<'_, AppState>, station: Station) -> Cmd<Station> {
    with_book(&state, |lb| lb.set_station(&station))
}

// ---------- 2.2 ADIF import ----------

fn read_adif(path: &str) -> AppResult<Vec<u8>> {
    let m = std::fs::metadata(path).map_err(|e| AppError::from(e).with_context("Cannot read the ADIF file"))?;
    if m.len() > emcomm_core::adif_import::MAX_FILE_BYTES {
        return Err(AppError::validation("adif", "The file is larger than 64 MB; split it and import the parts"));
    }
    std::fs::read(path).map_err(|e| AppError::from(e).with_context("Cannot read the ADIF file"))
}

#[tauri::command]
async fn pick_adif_file(app: AppHandle) -> Cmd<Option<String>> {
    let filters = [DialogFilter { name: "ADIF (.adi, .adif)".into(), extensions: vec!["adi".into(), "adif".into()] }];
    #[cfg(not(target_os = "android"))]
    return Ok(file_dialog(&app, "Import ADIF", &filters, None).blocking_pick_file().and_then(to_path));
    #[cfg(target_os = "android")]
    android::pick_document(&app, &filters[0].extensions)
}

#[tauri::command]
async fn adif_preview(
    state: State<'_, AppState>,
    path: String,
    options: emcomm_core::adif_import::ImportOptions,
) -> Cmd<emcomm_core::adif_import::AdifPreview> {
    let bytes = read_adif(&path)?;
    let name = Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    with_book(&state, |lb| lb.adif_preview(&bytes, &name, &options))
}

#[tauri::command]
async fn adif_import(
    state: State<'_, AppState>,
    path: String,
    expected_sha256: String,
    selected: Vec<String>,
    options: emcomm_core::adif_import::ImportOptions,
) -> Cmd<emcomm_core::adif_import::AdifImportResult> {
    let bytes = read_adif(&path)?;
    let name = Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let op = operator_snapshot(&state)?;
    with_book(&state, |lb| {
        let who = lb.ensure_operator(&op)?;
        lb.adif_import(&bytes, &name, &expected_sha256, &selected, &options, &who)
    })
}

#[tauri::command]
async fn entry_links(state: State<'_, AppState>, entry_id: String, include_removed: bool) -> Cmd<Vec<emcomm_core::links::LinkView>> {
    with_book(&state, |lb| lb.entry_links(&entry_id, include_removed))
}

#[tauri::command]
async fn add_link(
    state: State<'_, AppState>,
    source_id: String,
    target_id: String,
    relation: emcomm_core::links::Relation,
    note: String,
) -> Cmd<emcomm_core::links::LinkView> {
    let op = operator_snapshot(&state)?;
    with_book(&state, |lb| {
        let who = lb.ensure_operator(&op)?;
        lb.add_link(&source_id, &target_id, relation, &note, &who)
    })
}

#[tauri::command]
async fn remove_link(state: State<'_, AppState>, link_id: String) -> Cmd<()> {
    let op = operator_snapshot(&state)?;
    with_book(&state, |lb| {
        let who = lb.ensure_operator(&op)?;
        lb.remove_link(&link_id, &who)
    })
}

#[tauri::command]
async fn list_directory(state: State<'_, AppState>, query: String) -> Cmd<Vec<emcomm_core::directory::DirectoryEntry>> {
    with_book(&state, |lb| lb.list_directory(&query))
}

#[tauri::command]
async fn save_directory(
    state: State<'_, AppState>,
    id: Option<String>,
    input: emcomm_core::directory::DirectoryInput,
) -> Cmd<emcomm_core::directory::DirectorySaveResult> {
    with_book(&state, |lb| lb.save_directory(id.as_deref(), &input))
}

#[tauri::command]
async fn delete_directory(state: State<'_, AppState>, id: String) -> Cmd<()> {
    with_book(&state, |lb| lb.delete_directory(&id))
}

#[tauri::command]
async fn mark_directory_used(state: State<'_, AppState>, id: String, entry_id: String) -> Cmd<()> {
    with_book(&state, |lb| lb.mark_directory_used(&id, &entry_id))
}

#[tauri::command]
async fn recent_values(
    state: State<'_, AppState>,
    fields: Vec<emcomm_core::recents::RecentField>,
) -> Cmd<std::collections::BTreeMap<emcomm_core::recents::RecentField, Vec<String>>> {
    with_book(&state, |lb| lb.recent_values(&fields))
}

#[tauri::command]
async fn operator_sessions(state: State<'_, AppState>) -> Cmd<Vec<OperatorSession>> {
    with_book(&state, |lb| lb.operator_sessions())
}

#[tauri::command]
async fn active_operator(state: State<'_, AppState>) -> Cmd<Option<OperatorSession>> {
    with_book(&state, |lb| lb.active_operator())
}

/// Hand the open logbook's station to another local profile. Profiles stay local; the session keeps a
/// snapshot, so the logbook remains readable on computers without that profile.
#[tauri::command]
async fn switch_operator(state: State<'_, AppState>, profile_id: String, at_utc: Option<String>, note: String) -> Cmd<OperatorSession> {
    let incoming = OperatorSnapshot::from(&lock(&state.app).get_profile(&profile_id)?);
    with_book(&state, |lb| lb.switch_operator(&incoming, at_utc.as_deref(), &note))
}

// ---------- 2.3: backup health and verification ----------

fn backup_health_for(s: &AppState, logbook_id: &str, live_dir: &Path) -> AppResult<backup_health::BackupHealth> {
    let (settings, runs) = {
        let db = lock(&s.app);
        (db.settings()?, db.backup_runs(logbook_id, 50)?)
    };
    let dest = settings.backup.destination.clone();
    let check = dest.as_deref().map(|d| backup::check_destination(Path::new(d), live_dir));
    let latest = dest.as_deref().filter(|d| Path::new(d).is_dir()).and_then(|d| backup_health::latest_backup(Path::new(d), logbook_id));
    let verifications = backup_health::VerificationStore::new(&s.data_dir).for_logbook(logbook_id);
    let live_marker = snapshot::live_data_marker(live_dir).ok();
    Ok(backup_health::assess(backup_health::HealthInputs {
        destination: dest.as_deref(),
        destination_check: check,
        runs: &runs,
        latest_backup: latest,
        verifications: &verifications,
        live_marker,
        last_unchanged_check_utc: lock(&s.last_unchanged_check).clone(),
        interval_minutes: settings.backup.interval_minutes,
        on_close: settings.backup.on_close,
        now: chrono::Utc::now(),
    }))
}

#[tauri::command]
async fn get_backup_health(app: AppHandle) -> Cmd<backup_health::BackupHealth> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = app.state::<AppState>();
        let (id, dir) = lock(&s.book).as_ref().map(|lb| (lb.id.clone(), lb.dir.clone())).ok_or_else(no_book)?;
        backup_health_for(&s, &id, &dir)
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
}

/// Verify the newest backup of the active logbook, off the UI thread. Read-only: the backup is never
/// modified, pruned, restored or repaired. Holds the backup lock so retention cannot remove the folder
/// while it is being read.
#[tauri::command]
async fn verify_latest_backup(app: AppHandle) -> Cmd<backup_health::VerifyReport> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = app.state::<AppState>();
        let (id, dir) = lock(&s.book).as_ref().map(|lb| (lb.id.clone(), lb.dir.clone())).ok_or_else(no_book)?;
        let dest = lock(&s.app)
            .settings()?
            .backup
            .destination
            .ok_or_else(|| AppError::validation("destination", "No backup destination is configured"))?;
        if !Path::new(&dest).is_dir() {
            return Err(AppError::new(ErrorCode::StorageUnavailable, "The backup destination is not available (drive disconnected?)"));
        }
        if s.verifying.swap(true, Ordering::SeqCst) {
            return Err(AppError::new(ErrorCode::Conflict, "A verification is already running"));
        }
        let result = (|| {
            let _guard = lock(&s.backup_lock);
            let latest = backup_health::latest_backup(Path::new(&dest), &id)
                .ok_or_else(|| AppError::not_found("No backup of this logbook was found at the destination"))?;
            let report = backup_health::verify_backup(Path::new(&latest.path));
            backup_health::VerificationStore::new(&s.data_dir).record(backup_health::VerificationRecord::from_report(&id, &report))?;
            Ok(report)
        })();
        s.verifying.store(false, Ordering::SeqCst);
        if let Ok(h) = backup_health_for(&s, &id, &dir) {
            let _ = app.emit("backup-health", h);
        }
        result
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
}

// ---------- 2.3: handover summary and traffic lifecycle (read-only) ----------

#[tauri::command]
async fn handover_preview(
    state: State<'_, AppState>,
    incoming_profile_id: Option<String>,
    at_utc: Option<String>,
    note: String,
) -> Cmd<emcomm_core::handover::HandoverSummary> {
    let incoming = match incoming_profile_id.filter(|p| !p.is_empty()) {
        Some(p) => Some(OperatorSnapshot::from(&lock(&state.app).get_profile(&p)?)),
        None => None,
    };
    with_book(&state, |lb| emcomm_core::handover::preview(&lb.conn, incoming, at_utc.as_deref(), &note))
}

#[tauri::command]
async fn shift_summary(state: State<'_, AppState>, session_id: String) -> Cmd<emcomm_core::handover::HandoverSummary> {
    with_book(&state, |lb| emcomm_core::handover::reconstruct(&lb.conn, &session_id))
}

#[tauri::command]
async fn entry_lifecycle(state: State<'_, AppState>, id: String) -> Cmd<emcomm_core::lifecycle::Lifecycle> {
    with_book(&state, |lb| emcomm_core::lifecycle::entry_lifecycle(&lb.conn, &id))
}

#[tauri::command]
async fn latest_handling_changes(state: State<'_, AppState>, ids: Vec<String>) -> Cmd<Vec<emcomm_core::lifecycle::LatestHandlingChange>> {
    with_book(&state, |lb| emcomm_core::lifecycle::latest_handling_changes(&lb.conn, &ids))
}

// ---------- GPS (2.1): one fix on request, never tracking ----------

#[tauri::command]
async fn gps_sources() -> Cmd<Vec<emcomm_core::gps::GpsSource>> {
    let mut out = Vec::new();
    // Android tablet: the tablet's own location only (no external receivers, by design).
    #[cfg(target_os = "android")]
    out.push(emcomm_core::gps::GpsSource {
        provider: "os".into(),
        path: String::new(),
        label: "Tablet location".into(),
        serial_kind: None,
        detail: "The tablet's GPS when it has one (works offline); otherwise Android's network location".into(),
    });
    #[cfg(not(target_os = "android"))]
    out.extend(tauri::async_runtime::spawn_blocking(emcomm_core::gps::list_serial).await.unwrap_or_default());
    if cfg!(target_os = "linux") {
        out.push(emcomm_core::gps::GpsSource {
            provider: "gpsd".into(),
            path: "127.0.0.1:2947".into(),
            label: "gpsd on this computer".into(),
            serial_kind: None,
            detail: "Uses the receiver configured in the gpsd service".into(),
        });
    }
    if emcomm_core::gps::os::SUPPORTED {
        out.push(emcomm_core::gps::GpsSource {
            provider: "os".into(),
            path: String::new(),
            label: emcomm_core::gps::os::LABEL.into(),
            serial_kind: None,
            detail: "May use Wi-Fi or a stored position, so it is labelled OS location, not GPS".into(),
        });
    }
    Ok(out)
}

/// Acquire one fix. Runs off the UI thread; `gps_cancel` stops it and every path releases the device.
#[tauri::command]
async fn gps_acquire(app: AppHandle, state: State<'_, AppState>, req: emcomm_core::gps::GpsRequest) -> Cmd<emcomm_core::gps::GpsOutcome> {
    if state.gps_busy.swap(true, Ordering::SeqCst) {
        return Ok(emcomm_core::gps::GpsOutcome::fail("device_busy", "A GPS fix is already being acquired"));
    }
    state.gps_cancel.store(false, Ordering::SeqCst);
    let (busy, cancel) = (state.gps_busy.clone(), state.gps_cancel.clone());
    let out = tauri::async_runtime::spawn_blocking(move || {
        let o = match req.provider.as_str() {
            #[cfg(not(target_os = "android"))]
            "serial" => emcomm_core::gps::acquire_serial(&req, &cancel),
            "gpsd" => emcomm_core::gps::acquire_gpsd(&req, &cancel),
            #[cfg(target_os = "android")]
            "os" => android::tablet_location(&app, &req),
            #[cfg(not(target_os = "android"))]
            "os" => {
                let main = |f: Box<dyn FnOnce() + Send>| {
                    let _ = app.run_on_main_thread(f);
                };
                emcomm_core::gps::os::acquire(&req, &cancel, &main)
            }
            other => emcomm_core::gps::GpsOutcome::fail("unavailable", format!("Unknown GPS source '{other}'")),
        };
        busy.store(false, Ordering::SeqCst);
        o
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?;
    Ok(out)
}

#[tauri::command]
async fn gps_cancel(app: AppHandle, state: State<'_, AppState>) -> Cmd<()> {
    state.gps_cancel.store(true, Ordering::SeqCst);
    #[cfg(target_os = "android")]
    android::cancel_tablet_location(&app);
    #[cfg(not(target_os = "android"))]
    let _ = app;
    Ok(())
}

// ---------- operational periods and ICS forms (V2) ----------

#[tauri::command]
async fn list_periods(state: State<'_, AppState>) -> Cmd<Vec<OperationalPeriod>> {
    with_book(&state, |lb| lb.periods())
}

#[tauri::command]
async fn current_period(state: State<'_, AppState>) -> Cmd<Option<OperationalPeriod>> {
    with_book(&state, |lb| lb.current_period())
}

#[tauri::command]
async fn save_period(state: State<'_, AppState>, id: Option<String>, input: OperationalPeriodInput) -> Cmd<PeriodSaveResult> {
    with_book(&state, |lb| lb.save_period(id.as_deref(), &input))
}

#[tauri::command]
async fn set_period_void(state: State<'_, AppState>, id: String, void: bool) -> Cmd<OperationalPeriod> {
    with_book(&state, |lb| lb.set_period_void(&id, void))
}

#[tauri::command]
async fn period_resources(state: State<'_, AppState>, op_id: String) -> Cmd<Vec<OpResource>> {
    with_book(&state, |lb| lb.period_resources(&op_id))
}

#[tauri::command]
async fn set_period_resources(state: State<'_, AppState>, op_id: String, rows: Vec<OpResource>) -> Cmd<Vec<OpResource>> {
    with_book(&state, |lb| lb.set_period_resources(&op_id, &rows))
}

#[tauri::command]
async fn period_plan(state: State<'_, AppState>, op_id: String) -> Cmd<ChannelPlan> {
    with_book(&state, |lb| lb.period_plan(&op_id))
}

#[tauri::command]
async fn set_period_plan(state: State<'_, AppState>, op_id: String, plan: ChannelPlan) -> Cmd<ChannelPlan> {
    with_book(&state, |lb| lb.set_period_plan(&op_id, &plan))
}

/// Another local logbook's database, opened read-only (never locked, migrated or written).
fn other_logbook_db(state: &AppState, logbook_id: &str) -> AppResult<emcomm_core::rusqlite::Connection> {
    let row = lock(&state.app)
        .list_index()?
        .into_iter()
        .find(|r| r.id == logbook_id)
        .ok_or_else(|| AppError::not_found("Logbook not in list"))?;
    Logbook::open_read_only(&Path::new(&row.path).join(emcomm_core::logbook::DB_FILE))
}

#[tauri::command]
async fn plan_sources(state: State<'_, AppState>, logbook_id: String) -> Cmd<Vec<PlanSource>> {
    let conn = other_logbook_db(&state, &logbook_id)?;
    periods::plan_sources(&conn)
}

#[tauri::command]
async fn read_plan_from(state: State<'_, AppState>, logbook_id: String, op_id: String) -> Cmd<ChannelPlan> {
    let conn = other_logbook_db(&state, &logbook_id)?;
    periods::read_plan(&conn, &op_id)
}

#[tauri::command]
async fn form213_preview(state: State<'_, AppState>, req: forms::Form213Request) -> Cmd<forms::FormPreview> {
    with_book(&state, |lb| forms::preview_213(&lb.conn, &lb.info()?, &req))
}

#[tauri::command]
async fn form214_preview(state: State<'_, AppState>, req: forms::Form214Request) -> Cmd<forms::FormPreview> {
    with_book(&state, |lb| forms::preview_214(&lb.conn, &lb.info()?, &req))
}

/// Build an ICS form from a read-only connection off the UI thread, then publish it atomically.
async fn run_form(
    app: &AppHandle,
    state: &AppState,
    dest: String,
    build: impl FnOnce(&emcomm_core::rusqlite::Connection, &LogbookInfo, &Path) -> AppResult<forms::FormResult> + Send + 'static,
) -> Cmd<forms::FormResult> {
    let (db_path, info) = with_book(state, |lb| Ok((lb.db_path(), lb.info()?)))?;
    let d = dest.clone();
    let r = tauri::async_runtime::spawn_blocking(move || {
        let conn = Logbook::open_read_only(&db_path)?;
        build(&conn, &info, Path::new(&d))
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))
    .and_then(|r| r);
    let r = delivered(app, &dest, r)?;
    lock(&state.reveal_allow).insert(PathBuf::from(dest));
    Ok(r)
}

#[tauri::command]
async fn export_form213(app: AppHandle, state: State<'_, AppState>, req: forms::Form213Request, dest: String) -> Cmd<forms::FormResult> {
    run_form(&app, &state, dest, move |c, i, p| forms::export_213(c, i, &req, p)).await
}

#[tauri::command]
async fn form309_preview(state: State<'_, AppState>, req: ics309::Form309Request) -> Cmd<forms::FormPreview> {
    with_book(&state, |lb| ics309::preview_309(&lb.conn, &lb.info()?, &req))
}

#[tauri::command]
async fn export_form309(app: AppHandle, state: State<'_, AppState>, req: ics309::Form309Request, dest: String) -> Cmd<forms::FormResult> {
    run_form(&app, &state, dest, move |c, i, p| ics309::export_309(c, i, &req, p)).await
}

#[tauri::command]
async fn export_form214(app: AppHandle, state: State<'_, AppState>, req: forms::Form214Request, dest: String) -> Cmd<forms::FormResult> {
    run_form(&app, &state, dest, move |c, i, p| forms::export_214(c, i, &req, p)).await
}

// ---------- app lifecycle ----------

#[tauri::command]
async fn quit_app(app: AppHandle, state: State<'_, AppState>) -> Cmd<()> {
    stop_scheduler(&state);
    if let Some(lb) = lock(&state.book).take() {
        let _ = refresh_index(&state, &lb);
        let _ = lb.close();
    }
    state.allow_exit.store(true, Ordering::SeqCst);
    #[cfg(target_os = "android")]
    {
        // Finish the activity; MainActivity ends the process once it is destroyed.
        android::exit_app(&app)?;
        return Ok(());
    }
    #[allow(unreachable_code)]
    {
        app.exit(0);
        Ok(())
    }
}

/// The app went to the background (Android). Drafts were flushed by the UI; make the live file
/// self-contained, refresh the logbook list's counts, and take an automatic backup if anything
/// changed — the process may be killed without any close event.
#[tauri::command]
async fn app_backgrounded(app: AppHandle, state: State<'_, AppState>) -> Cmd<BackupOutcome> {
    {
        let g = lock(&state.book);
        let Some(lb) = g.as_ref() else {
            return Ok(BackupOutcome { ok: true, skipped: true, path: None, error: None });
        };
        let _ = lb.checkpoint();
        let _ = refresh_index(&state, lb);
    }
    let a = app.clone();
    let out = tauri::async_runtime::spawn_blocking(move || run_backup_kind(&a, "auto", true))
        .await
        .map_err(|e| AppError::internal(e.to_string()))?;
    Ok(out)
}

/// Storage locations offered instead of folder pickers on Android; null on desktop.
#[cfg(target_os = "android")]
#[tauri::command]
async fn device_locations(app: AppHandle, state: State<'_, AppState>) -> Cmd<Option<android::Locations>> {
    android::locations(&app, &state.data_dir).map(Some)
}

#[cfg(not(target_os = "android"))]
#[tauri::command]
async fn device_locations() -> Cmd<Option<()>> {
    Ok(None)
}

/// Match Android's status/navigation bar area to the app theme. No-op on desktop.
#[tauri::command]
async fn set_system_bars(app: AppHandle, dark: bool, color: String) -> Cmd<()> {
    #[cfg(target_os = "android")]
    return android::set_system_bars(&app, dark, &color);
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, dark, color);
        Ok(())
    }
}

#[tauri::command]
async fn cancel_close(state: State<'_, AppState>) -> Cmd<()> {
    *lock(&state.close_requested_at) = None;
    Ok(())
}

/// Returns true if the close should proceed immediately (a second request while the UI is
/// unresponsive); otherwise asks the UI to flush drafts and run the close backup.
fn handle_close_request(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    if state.allow_exit.load(Ordering::SeqCst) {
        return true;
    }
    let mut g = lock(&state.close_requested_at);
    if let Some(t) = *g
        && t.elapsed() < Duration::from_secs(15)
        && t.elapsed() > Duration::from_secs(3)
    {
        return true;
    }
    *g = Some(Instant::now());
    let _ = app.emit("close-requested", ());
    false
}

/// WebView2 browser arguments: wry's defaults plus any supplied through
/// WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS (used by WebDriver/msedgedriver for automated testing),
/// with all --disable-features lists merged since Chromium honors only the last one.
#[cfg_attr(not(windows), allow(dead_code))]
fn webview2_args(extra: &str) -> String {
    let defaults = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";
    let mut features: Vec<String> = Vec::new();
    let mut rest: Vec<String> = Vec::new();
    for arg in defaults.split_whitespace().chain(extra.split_whitespace()) {
        match arg.strip_prefix("--disable-features=") {
            Some(list) => {
                for f in list.split(',').filter(|f| !f.is_empty()) {
                    if !features.iter().any(|x| x == f) {
                        features.push(f.to_string());
                    }
                }
            }
            None => rest.push(arg.to_string()),
        }
    }
    rest.push(format!("--disable-features={}", features.join(",")));
    rest.join(" ")
}

fn create_main_window(app: &tauri::App) -> tauri::Result<()> {
    let cfg = app.config().app.windows.iter().find(|w| w.label == "main").cloned().expect("main window config");
    #[allow(unused_mut)]
    let mut builder = tauri::WebviewWindowBuilder::from_config(app.handle(), &cfg)?;
    #[cfg(windows)]
    {
        let extra = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").unwrap_or_default();
        builder = builder.additional_browser_args(&webview2_args(&extra));
    }
    builder.build()?;
    Ok(())
}

#[cfg(desktop)]
/// The default 1440x900 window can exceed small displays (e.g. 1366x768 laptops); shrink to fit.
fn fit_window_to_screen(app: &AppHandle) {
    let Some(w) = app.get_webview_window("main") else { return };
    let Ok(Some(m)) = w.current_monitor() else { return };
    let scale = m.scale_factor();
    let area = m.work_area().size;
    let (aw, ah) = (area.width as f64 / scale, area.height as f64 / scale);
    let (cw, ch) = (1440.0_f64, 900.0_f64);
    if cw > aw * 0.96 || ch > ah * 0.96 {
        let size = tauri::LogicalSize::new(cw.min(aw * 0.96).max(640.0), ch.min(ah * 0.96).max(480.0));
        let _ = w.set_min_size(Some(tauri::LogicalSize::new(size.width.min(960.0), size.height.min(620.0))));
        let _ = w.set_size(size);
        // Position explicitly: center() can run before the resize is applied on macOS.
        let pos = m.work_area().position;
        let x = pos.x as f64 / scale + (aw - size.width).max(0.0) / 2.0;
        let y = pos.y as f64 / scale + (ah - size.height).max(0.0) / 2.0;
        let _ = w.set_position(tauri::LogicalPosition::new(x, y));
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_dialog::init());
    #[cfg(target_os = "android")]
    let builder = builder.plugin(android::init());
    builder
        .setup(|app| {
            // EMCOMM_DATA_DIR relocates local settings/profiles/drafts (testing, portable installs).
            let data_dir = match std::env::var_os("EMCOMM_DATA_DIR") {
                Some(d) if !d.is_empty() => PathBuf::from(d),
                _ => app.path().app_data_dir().map_err(|e| Box::new(AppError::internal(format!("No app data directory: {e}"))))?,
            };
            std::fs::create_dir_all(&data_dir)?;
            let db = AppDb::open(&data_dir).map_err(Box::new)?;
            app.manage(AppState {
                app: Mutex::new(db),
                book: Mutex::new(None),
                drafts: DraftStore::new(&data_dir.join("drafts")),
                data_dir,
                backup_lock: Mutex::new(()),
                backup_running: AtomicBool::new(false),
                scheduler_stop: Mutex::new(None),
                next_auto_due: Mutex::new(None),
                close_requested_at: Mutex::new(None),
                allow_exit: AtomicBool::new(false),
                reveal_allow: Mutex::new(HashSet::new()),
                gps_busy: Arc::new(AtomicBool::new(false)),
                gps_cancel: Arc::new(AtomicBool::new(false)),
                attach_cancel: Arc::new(AtomicBool::new(false)),
                last_unchanged_check: Mutex::new(None),
                verifying: AtomicBool::new(false),
            });
            create_main_window(app)?;
            #[cfg(desktop)]
            fit_window_to_screen(app.handle());
            #[cfg(desktop)]
            {
                let m = menu::build(app.handle())?;
                app.set_menu(m)?;
                app.on_menu_event(|app, event| {
                    let id = event.id().0.clone();
                    if id == "quit" {
                        if handle_close_request(app) {
                            app.state::<AppState>().allow_exit.store(true, Ordering::SeqCst);
                            app.exit(0);
                        }
                        return;
                    }
                    let _ = app.emit("menu", id);
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && !handle_close_request(window.app_handle())
            {
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_bootstrap,
            save_settings,
            check_folder,
            pick_folder,
            pick_file,
            pick_save_file,
            create_profile,
            update_profile,
            set_profile_active,
            list_profiles,
            list_presets,
            create_preset,
            update_preset,
            duplicate_preset,
            delete_preset,
            list_logbooks,
            create_logbook,
            open_logbook,
            open_logbook_folder,
            close_logbook,
            relink_logbook,
            forget_logbook,
            set_logbook_archived,
            rename_logbook,
            update_logbook_identity,
            update_logbook_settings,
            get_active,
            logbook_stats,
            storage_health,
            reveal_path,
            open_export,
            open_attachment,
            save_entry,
            save_changes,
            void_entry,
            restore_entry,
            get_entry,
            list_entries,
            entry_revisions,
            set_favorite,
            stage_attachment,
            discard_staged,
            pick_attachments,
            take_photo,
            describe_files,
            stage_attachment_path,
            cancel_attach,
            stage_attachment_bytes,
            attachment_thumbnail,
            save_attachment_as,
            staged_exists,
            list_templates,
            save_template,
            delete_template,
            save_draft,
            list_drafts,
            delete_draft,
            run_backup_now,
            get_backup_health,
            verify_latest_backup,
            handover_preview,
            shift_summary,
            entry_lifecycle,
            latest_handling_changes,
            get_backup_status,
            list_backups,
            preview_backup,
            restore_backup,
            export_package,
            preview_package,
            import_package,
            export_preview,
            export_review,
            form_review,
            run_export,
            gps_sources,
            gps_acquire,
            gps_cancel,
            get_station,
            set_station,
            recent_values,
            pick_adif_file,
            adif_preview,
            adif_import,
            entry_links,
            add_link,
            remove_link,
            list_directory,
            save_directory,
            delete_directory,
            mark_directory_used,
            operator_sessions,
            active_operator,
            switch_operator,
            list_periods,
            current_period,
            save_period,
            set_period_void,
            period_resources,
            set_period_resources,
            period_plan,
            set_period_plan,
            plan_sources,
            read_plan_from,
            form213_preview,
            form214_preview,
            export_form213,
            export_form214,
            form309_preview,
            export_form309,
            quit_app,
            cancel_close,
            app_backgrounded,
            device_locations,
            set_system_bars,
        ])
        .build(tauri::generate_context!())
        .expect("error while building EMCOMM LOG")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event
                && code.is_none()
                && !handle_close_request(app)
            {
                api.prevent_exit();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::webview2_args;

    #[test]
    fn webview2_args_merge_driver_flags() {
        assert_eq!(webview2_args(""), "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection");
        let a = webview2_args("--remote-debugging-port=0 --disable-features=IgnoreDuplicateNavs,msWebOOUI --enable-automation");
        assert!(a.contains("--remote-debugging-port=0") && a.contains("--enable-automation"));
        assert_eq!(a.matches("--disable-features=").count(), 1);
        assert!(a.ends_with("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,IgnoreDuplicateNavs"));
    }
}
