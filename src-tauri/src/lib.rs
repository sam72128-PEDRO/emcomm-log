//! Tauri shell: typed commands over `emcomm_core`, native menus, file dialogs (Rust-side, so the
//! webview needs no filesystem capability), backup scheduling, and the close workflow.

mod menu;

use emcomm_core::appdb::{AppDb, BackupRun};
use emcomm_core::backup::{self, BackupListing, BackupPreview};
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
    name: String,
    extensions: Vec<String>,
}

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
    if let Some(w) = app.get_webview_window("main") {
        d = d.set_parent(&w);
    }
    d
}

fn to_path(p: tauri_plugin_dialog::FilePath) -> Option<String> {
    p.into_path().ok().map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
async fn pick_folder(app: AppHandle, title: String, start: Option<String>) -> Cmd<Option<String>> {
    Ok(file_dialog(&app, &title, &[], start.as_deref()).blocking_pick_folder().and_then(to_path))
}

#[tauri::command]
async fn pick_file(app: AppHandle, title: String, filters: Vec<DialogFilter>) -> Cmd<Option<String>> {
    Ok(file_dialog(&app, &title, &filters, None).blocking_pick_file().and_then(to_path))
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
            Ok(r) if r.pruned > 0 => format!("Verified; removed {} old backup(s) per retention", r.pruned),
            Ok(_) => "Verified".into(),
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

fn os_open(path: &Path, reveal: bool) -> AppResult<()> {
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
async fn reveal_path(state: State<'_, AppState>, path: String) -> Cmd<()> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(AppError::new(ErrorCode::StorageUnavailable, "That location is not available"));
    }
    if !allowed_reveal_roots(&state)?.iter().any(|r| fsutil::is_within(&p, r)) {
        return Err(AppError::new(ErrorCode::PermissionDenied, "Only logbook, backup, and export locations can be revealed"));
    }
    os_open(&p, true)
}

/// Open a file this session exported (e.g. a PDF for printing) in the default application.
#[tauri::command]
async fn open_export(state: State<'_, AppState>, path: String) -> Cmd<()> {
    let p = PathBuf::from(&path);
    if !lock(&state.reveal_allow).contains(&p) || !p.is_file() {
        return Err(AppError::new(ErrorCode::PermissionDenied, "Only files exported in this session can be opened"));
    }
    os_open(&p, false)
}

#[tauri::command]
async fn open_attachment(state: State<'_, AppState>, attachment_id: String) -> Cmd<()> {
    let p = with_book(&state, |lb| lb.attachment_path(&attachment_id))?;
    os_open(&p, false)
}

// ---------- entries ----------

#[tauri::command]
async fn save_entry(state: State<'_, AppState>, input: EntryInput, request_id: String, draft_id: Option<String>) -> Cmd<SaveResult> {
    let op = operator_snapshot(&state)?;
    let (res, lbid) = with_book(&state, |lb| Ok((lb.save_new(&input, &request_id, &op)?, lb.id.clone())))?;
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
    let (res, lbid) =
        with_book(&state, |lb| Ok((lb.save_changes(&entry_id, base_revision, &input, &reason, &request_id, &op)?, lb.id.clone())))?;
    if let Some(d) = draft_id {
        let _ = state.drafts.delete(&lbid, &d);
    }
    Ok(res)
}

#[tauri::command]
async fn void_entry(state: State<'_, AppState>, entry_id: String, reason: String, request_id: String) -> Cmd<SaveResult> {
    let op = operator_snapshot(&state)?;
    with_book(&state, |lb| lb.void_entry(&entry_id, &reason, &request_id, &op))
}

#[tauri::command]
async fn restore_entry(state: State<'_, AppState>, entry_id: String, reason: String, request_id: String) -> Cmd<SaveResult> {
    let op = operator_snapshot(&state)?;
    with_book(&state, |lb| lb.restore_entry(&entry_id, &reason, &request_id, &op))
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
    let Some(path) = file_dialog(&app, "Attach a file", &[], None).blocking_pick_file().and_then(to_path) else { return Ok(None) };
    with_book(&state, |lb| lb.stage_attachment(Path::new(&path))).map(Some)
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

// ---------- packages ----------

/// Export the open logbook, or any listed logbook by id (read through a consistent snapshot).
#[tauri::command]
async fn export_package(state: State<'_, AppState>, id: Option<String>, dest: String) -> Cmd<ExportPackageResult> {
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
        .map_err(|e| AppError::internal(e.to_string()))??;
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

#[tauri::command]
async fn run_export(state: State<'_, AppState>, req: ExportRequest, dest: String) -> Cmd<ExportResult> {
    let (db_path, info) = with_book(&state, |lb| Ok((lb.db_path(), lb.info()?)))?;
    let d = dest.clone();
    let r = tauri::async_runtime::spawn_blocking(move || {
        let conn = Logbook::open_read_only(&db_path)?;
        export::run_export(&conn, &info, &req, Path::new(&d))
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))??;
    lock(&state.reveal_allow).insert(PathBuf::from(dest));
    Ok(r)
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
    .map_err(|e| AppError::internal(e.to_string()))??;
    lock(&state.reveal_allow).insert(PathBuf::from(dest));
    Ok(r)
}

#[tauri::command]
async fn export_form213(state: State<'_, AppState>, req: forms::Form213Request, dest: String) -> Cmd<forms::FormResult> {
    run_form(&state, dest, move |c, i, p| forms::export_213(c, i, &req, p)).await
}

#[tauri::command]
async fn form309_preview(state: State<'_, AppState>, req: ics309::Form309Request) -> Cmd<forms::FormPreview> {
    with_book(&state, |lb| ics309::preview_309(&lb.conn, &lb.info()?, &req))
}

#[tauri::command]
async fn export_form309(state: State<'_, AppState>, req: ics309::Form309Request, dest: String) -> Cmd<forms::FormResult> {
    run_form(&state, dest, move |c, i, p| ics309::export_309(c, i, &req, p)).await
}

#[tauri::command]
async fn export_form214(state: State<'_, AppState>, req: forms::Form214Request, dest: String) -> Cmd<forms::FormResult> {
    run_form(&state, dest, move |c, i, p| forms::export_214(c, i, &req, p)).await
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
    app.exit(0);
    Ok(())
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
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
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
            });
            create_main_window(app)?;
            fit_window_to_screen(app.handle());
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
            staged_exists,
            list_templates,
            save_template,
            delete_template,
            save_draft,
            list_drafts,
            delete_draft,
            run_backup_now,
            get_backup_status,
            list_backups,
            preview_backup,
            restore_backup,
            export_package,
            preview_package,
            import_package,
            export_preview,
            run_export,
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
