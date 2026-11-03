//! Android bridge: calls into `EmcommPlugin.kt` (system file picker, "Save to…", share sheet,
//! opening files in other apps, microSD discovery, system bars, exit).
//!
//! Rust only ever works on real paths inside app storage. Documents the user picks are copied
//! into the cache by the Kotlin side; exports are written to the cache and then copied to the
//! document the user chose, so the core's temp-file + rename + fsync discipline is unchanged.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use emcomm_core::error::{AppError, AppResult, ErrorCode};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
use tauri::{AppHandle, Manager, Wry};

const PLUGIN_PACKAGE: &str = "org.emcommlog.tablet";

pub struct Bridge(PluginHandle<Wry>);

/// Export destinations chosen with "Save to…", keyed by the cache path the export is written to.
static PENDING_SAVES: Mutex<Option<HashMap<PathBuf, String>>> = Mutex::new(None);

pub fn init() -> TauriPlugin<Wry> {
    Builder::new("emcomm-android")
        .setup(|app, api| {
            let handle = api.register_android_plugin(PLUGIN_PACKAGE, "EmcommPlugin")?;
            app.manage(Bridge(handle));
            Ok(())
        })
        .build()
}

fn bridge(app: &AppHandle) -> AppResult<tauri::State<'_, Bridge>> {
    app.try_state::<Bridge>().ok_or_else(|| AppError::internal("Android bridge not loaded"))
}

fn call<T: serde::de::DeserializeOwned>(app: &AppHandle, cmd: &str, args: serde_json::Value) -> AppResult<T> {
    bridge(app)?.0.run_mobile_plugin::<T>(cmd, args).map_err(|e| {
        let msg = e.to_string();
        AppError::new(ErrorCode::Io, msg)
    })
}

#[derive(Deserialize)]
struct Picked {
    path: Option<String>,
}

#[derive(Deserialize)]
struct Created {
    uri: Option<String>,
}

#[derive(Deserialize)]
struct Empty {}

#[derive(Deserialize)]
struct RemovableDir {
    path: String,
    state: String,
}

#[derive(Deserialize)]
struct StorageInfo {
    removable: Vec<RemovableDir>,
    cache: String,
}

/// Guess a MIME type from a file extension (for pickers and "Open with").
pub fn mime_for(name: &str) -> &'static str {
    let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "csv" => "text/csv",
        "txt" => "text/plain",
        "adi" | "adif" => "text/plain",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "json" => "application/json",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

/// Let the user pick a document; returns a private copy's path, or None if cancelled.
pub fn pick_document(app: &AppHandle, extensions: &[String]) -> AppResult<Option<String>> {
    // Packages (.emlog) have no registered MIME type, so only filter for well-known types.
    let mimes: Vec<&str> = extensions.iter().map(|e| mime_for(&format!("x.{e}"))).collect();
    let mimes = if mimes.contains(&"application/octet-stream") || mimes.is_empty() { vec!["*/*"] } else { mimes };
    let r: Picked = call(app, "pickDocument", json!({ "mimeTypes": mimes }))?;
    Ok(r.path)
}

fn cache_dir(app: &AppHandle) -> AppResult<PathBuf> {
    app.path().app_cache_dir().map_err(|e| AppError::internal(format!("No cache directory: {e}")))
}

/// Folder shared with other apps through the FileProvider (`res/xml/file_paths.xml`).
pub fn shared_dir(app: &AppHandle, sub: &str) -> AppResult<PathBuf> {
    let d = cache_dir(app)?.join("shared").join(sub);
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

/// "Save to…": ask where the export should go. Returns the cache path to write it to, or None.
pub fn choose_save_target(app: &AppHandle, file_name: &str) -> AppResult<Option<String>> {
    let r: Created = call(app, "createDocument", json!({ "name": file_name, "mime": mime_for(file_name) }))?;
    let Some(uri) = r.uri else { return Ok(None) };
    let dir = shared_dir(app, &format!("exports/{}", emcomm_core::fsutil::short_uuid(&emcomm_core::fsutil::new_uuid())))?;
    let path = dir.join(file_name);
    PENDING_SAVES.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(HashMap::new).insert(path.clone(), uri);
    Ok(Some(path.to_string_lossy().to_string()))
}

/// After an export was written to `dest`, copy it to the document the user chose.
pub fn deliver_export(app: &AppHandle, dest: &str) -> AppResult<()> {
    let uri = PENDING_SAVES.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(HashMap::new).remove(Path::new(dest));
    if let Some(uri) = uri {
        let _: Empty = call(app, "copyToUri", json!({ "path": dest, "uri": uri }))?;
    }
    Ok(())
}

/// Forget a pending "Save to…" when the export itself failed.
pub fn forget_export(dest: &str) {
    if let Some(m) = PENDING_SAVES.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
        m.remove(Path::new(dest));
    }
}

/// Make `path` shareable: files outside the shared cache are copied into it first.
fn shareable(app: &AppHandle, path: &Path) -> AppResult<PathBuf> {
    let shared = cache_dir(app)?.join("shared");
    if emcomm_core::fsutil::is_within(path, &shared) {
        return Ok(path.to_path_buf());
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let dir = shared_dir(app, &format!("view/{}", emcomm_core::fsutil::short_uuid(&emcomm_core::fsutil::new_uuid())))?;
    let dst = dir.join(emcomm_core::fsutil::safe_component(name, 120));
    std::fs::copy(path, &dst)?;
    Ok(dst)
}

/// Open a file in another app ("Open with").
pub fn view_file(app: &AppHandle, path: &Path) -> AppResult<()> {
    if path.is_dir() {
        return Err(AppError::new(ErrorCode::Validation, "Folders cannot be opened in other apps on Android"));
    }
    let p = shareable(app, path)?;
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let _: Empty = call(app, "viewFile", json!({ "path": p, "mime": mime_for(name), "title": "Open with" }))?;
    Ok(())
}

/// Hand a file to the Android share sheet (Drive, email, Nearby Share, Files…).
pub fn share_file(app: &AppHandle, path: &Path) -> AppResult<()> {
    if path.is_dir() {
        return Err(AppError::new(ErrorCode::Validation, "Folders cannot be shared on Android; export a package instead"));
    }
    let p = shareable(app, path)?;
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let _: Empty = call(app, "shareFile", json!({ "path": p, "mime": mime_for(name), "title": "Share" }))?;
    Ok(())
}

#[derive(Serialize)]
pub struct Locations {
    /// Live logbooks: app-private internal storage (the only place with real paths and locks).
    logbooks: String,
    /// Backups on internal storage.
    tablet_backups: String,
    /// Backups on the microSD card (app-specific folder), when a card is mounted.
    sd_backups: Option<String>,
    /// Android's state for the card ("mounted", "unmounted", "removed"…), if one was ever seen.
    sd_state: Option<String>,
}

pub fn locations(app: &AppHandle, data_dir: &Path) -> AppResult<Locations> {
    let logbooks = data_dir.join("logbooks");
    let tablet_backups = data_dir.join("backups");
    std::fs::create_dir_all(&logbooks)?;
    std::fs::create_dir_all(&tablet_backups)?;
    let info: StorageInfo = call(app, "storageInfo", json!({}))?;
    let _ = info.cache;
    let sd = info.removable.into_iter().next();
    let sd_state = sd.as_ref().map(|d| d.state.clone());
    let sd_backups = match sd {
        Some(d) if d.state == "mounted" => {
            let p = PathBuf::from(d.path).join("EMCOMM LOG Backups");
            std::fs::create_dir_all(&p).ok().map(|_| p.to_string_lossy().to_string())
        }
        _ => None,
    };
    Ok(Locations {
        logbooks: logbooks.to_string_lossy().to_string(),
        tablet_backups: tablet_backups.to_string_lossy().to_string(),
        sd_backups,
        sd_state,
    })
}

pub fn set_system_bars(app: &AppHandle, dark: bool, color: &str) -> AppResult<()> {
    let _: Empty = call(app, "setSystemBars", json!({ "dark": dark, "color": color }))?;
    Ok(())
}

pub fn exit_app(app: &AppHandle) -> AppResult<()> {
    let _: Empty = call(app, "exitApp", json!({}))?;
    Ok(())
}
