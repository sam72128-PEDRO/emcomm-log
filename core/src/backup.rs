//! Independent backups (automatic, close-time, manual), retention, and restore as a new logbook.

use crate::error::{AppError, AppResult, ErrorCode};
use crate::fsutil::{self, now_utc, short_uuid};
use crate::logbook::{self, ATTACHMENTS_DIR, DB_FILE, DESCRIPTOR_FILE, Descriptor};
use crate::migrate::{self, LOGBOOK_MIGRATIONS};
use crate::models::{BackupSettings, LogbookType};
use crate::snapshot::{self, BACKUP_FORMAT, MANIFEST_FILE, Manifest};
use chrono::{DateTime, Datelike, Utc};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupResult {
    pub path: String,
    pub created_at_utc: String,
    pub entry_count: i64,
    pub data_marker: String,
    pub pruned: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupListing {
    pub path: String,
    pub logbook_id: String,
    pub name: String,
    pub logbook_type: LogbookType,
    pub created_at_utc: String,
    pub kind: String,
    pub entry_count: i64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupPreview {
    pub path: String,
    pub logbook_id: String,
    pub name: String,
    pub logbook_type: LogbookType,
    pub created_at_utc: String,
    pub kind: String,
    pub entry_count: i64,
    pub max_sequence: i64,
    pub attachment_count: usize,
    pub first_event_utc: Option<String>,
    pub last_event_utc: Option<String>,
}

/// The destination must be an existing writable folder outside the live logbook.
pub fn check_destination(dest_root: &Path, live_dir: &Path) -> AppResult<()> {
    if !dest_root.is_dir() {
        return Err(AppError::new(ErrorCode::StorageUnavailable, "Backup destination is not available (drive disconnected?)"));
    }
    if fsutil::is_within(dest_root, live_dir) {
        return Err(AppError::new(ErrorCode::Validation, "Backup destination must not be inside the live logbook folder"));
    }
    if fsutil::is_within(live_dir, dest_root) && fsutil::canonical_or_self(dest_root) == fsutil::canonical_or_self(live_dir) {
        return Err(AppError::new(ErrorCode::Validation, "Backup destination must not be the live logbook folder"));
    }
    fsutil::check_writable(dest_root)
}

fn book_dir_name(name: &str, id: &str) -> String {
    format!("{}--{}", fsutil::safe_component(name, 60), short_uuid(id))
}

/// Create one verified backup. The backup is assembled in a temp directory, every file is
/// re-hashed and the database integrity-checked, and only then is it renamed into place.
/// A failure leaves earlier backups untouched; retention runs only after a success.
pub fn run_backup(live_dir: &Path, dest_root: &Path, kind: &str, retention: &BackupSettings) -> AppResult<BackupResult> {
    if !["auto", "close", "manual"].contains(&kind) {
        return Err(AppError::internal("unknown backup kind"));
    }
    check_destination(dest_root, live_dir)?;
    let desc = logbook::read_descriptor(live_dir)
        .ok_or_else(|| AppError::new(ErrorCode::StorageUnavailable, "Live logbook descriptor is missing"))?;
    let book_dir = dest_root.join(book_dir_name(&desc.name, &desc.logbook_id));
    let book_dir = find_existing_book_dir(dest_root, &desc.logbook_id).unwrap_or(book_dir);
    fs::create_dir_all(&book_dir)?;
    let stamp = Utc::now().format("%Y%m%dT%H%M%S%3fZ").to_string();
    let tmp = book_dir.join(format!(".tmp-{stamp}-{kind}-{}", short_uuid(&fsutil::new_uuid())));
    let final_dir = book_dir.join(format!("{stamp}-{kind}"));
    let result = (|| -> AppResult<BackupResult> {
        fs::create_dir(&tmp)?;
        let snap = snapshot::take_snapshot(live_dir, &tmp)?;
        let manifest = snapshot::manifest_for(&snap, BACKUP_FORMAT, Some(kind));
        fsutil::atomic_write(&tmp.join(MANIFEST_FILE), serde_json::to_string_pretty(&manifest)?.as_bytes())?;
        crate::faults::check("backup.before_verify")?;
        snapshot::verify_dir(&tmp, BACKUP_FORMAT).map_err(|e| e.with_context("Backup verification failed"))?;
        fsutil::sync_dir(&tmp)?;
        fs::rename(&tmp, &final_dir)?;
        fsutil::sync_dir(&book_dir)?;
        Ok(BackupResult {
            path: final_dir.to_string_lossy().to_string(),
            created_at_utc: manifest.created_at_utc,
            entry_count: snap.entry_count,
            data_marker: snap.data_marker,
            pruned: 0,
        })
    })();
    match result {
        Ok(mut r) => {
            r.pruned = apply_retention(&book_dir, retention, &final_dir).unwrap_or(0);
            Ok(r)
        }
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp);
            Err(e)
        }
    }
}

fn find_existing_book_dir(dest_root: &Path, logbook_id: &str) -> Option<PathBuf> {
    let suffix = format!("--{}", short_uuid(logbook_id));
    fs::read_dir(dest_root)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.is_dir() && p.file_name().map(|n| n.to_string_lossy().ends_with(&suffix)).unwrap_or(false))
}

fn read_manifest(dir: &Path) -> Option<Manifest> {
    let m: Manifest = serde_json::from_slice(&fs::read(dir.join(MANIFEST_FILE)).ok()?).ok()?;
    (m.format == BACKUP_FORMAT).then_some(m)
}

/// Decide which backups to keep: the newest `keep_recent`, the newest of each of the last
/// `keep_daily` days, and the newest of each of the last `keep_weekly` ISO weeks.
pub fn retention_keep(mut items: Vec<(DateTime<Utc>, PathBuf)>, s: &BackupSettings) -> HashSet<PathBuf> {
    items.sort_by_key(|i| std::cmp::Reverse(i.0));
    let mut keep = HashSet::new();
    for (_, p) in items.iter().take(s.keep_recent.max(1) as usize) {
        keep.insert(p.clone());
    }
    let mut days = Vec::new();
    let mut weeks = Vec::new();
    for (t, p) in &items {
        let day = t.date_naive();
        if !days.contains(&day) && days.len() < s.keep_daily as usize {
            days.push(day);
            keep.insert(p.clone());
        }
        let w = t.iso_week();
        let wk = (w.year(), w.week());
        if !weeks.contains(&wk) && weeks.len() < s.keep_weekly as usize {
            weeks.push(wk);
            keep.insert(p.clone());
        }
    }
    keep
}

/// Delete only completed backups (valid manifest) not selected for keeping, plus stale temp
/// directories from failed attempts. Never deletes the backup just created.
pub fn apply_retention(book_dir: &Path, s: &BackupSettings, just_created: &Path) -> AppResult<usize> {
    let mut items = Vec::new();
    let mut removed = 0;
    for e in fs::read_dir(book_dir)?.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with(".tmp-") {
            let old =
                e.metadata().and_then(|m| m.modified()).map(|t| t.elapsed().map(|d| d.as_secs() > 3600).unwrap_or(false)).unwrap_or(false);
            if old && fs::remove_dir_all(&p).is_ok() {
                removed += 1;
            }
            continue;
        }
        if let Some(m) = read_manifest(&p)
            && let Some(t) = crate::validate::parse_utc(&m.created_at_utc)
        {
            items.push((t, p));
        }
    }
    let keep = retention_keep(items.clone(), s);
    for (_, p) in items {
        if !keep.contains(&p) && p != just_created && fs::remove_dir_all(&p).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// List completed backups found under a destination root (all logbooks).
pub fn list_backups(dest_root: &Path) -> AppResult<Vec<BackupListing>> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(dest_root) else {
        return Err(AppError::new(ErrorCode::StorageUnavailable, "Backup destination is not available"));
    };
    for book in rd.flatten().filter(|e| e.path().is_dir()) {
        let Ok(inner) = fs::read_dir(book.path()) else { continue };
        for b in inner.flatten() {
            if b.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            if let Some(m) = read_manifest(&b.path()) {
                out.push(BackupListing {
                    path: b.path().to_string_lossy().to_string(),
                    logbook_id: m.source.logbook_id.clone(),
                    name: m.source.name.clone(),
                    logbook_type: m.source.logbook_type,
                    created_at_utc: m.created_at_utc.clone(),
                    kind: m.backup_kind.clone().unwrap_or_default(),
                    entry_count: m.entry_count,
                    total_bytes: m.files.iter().map(|f| f.bytes).sum(),
                });
            }
        }
    }
    out.sort_by(|a, b| b.created_at_utc.cmp(&a.created_at_utc));
    Ok(out)
}

pub fn preview_backup(dir: &Path) -> AppResult<BackupPreview> {
    let m = snapshot::verify_dir(dir, BACKUP_FORMAT)?;
    let conn = logbook::Logbook::open_read_only(&dir.join(DB_FILE))?;
    let (first, last): (Option<String>, Option<String>) =
        conn.query_row("SELECT MIN(event_at_utc), MAX(event_at_utc) FROM entries WHERE status='active'", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
    Ok(BackupPreview {
        path: dir.to_string_lossy().to_string(),
        logbook_id: m.source.logbook_id,
        name: m.source.name,
        logbook_type: m.source.logbook_type,
        created_at_utc: m.created_at_utc,
        kind: m.backup_kind.unwrap_or_default(),
        entry_count: m.entry_count,
        max_sequence: m.max_sequence,
        attachment_count: m.files.len().saturating_sub(1),
        first_event_utc: first,
        last_event_utc: last,
    })
}

#[derive(Debug)]
pub struct InstallOutcome {
    pub path: PathBuf,
    pub logbook_id: String,
    pub source_uuid: String,
    pub new_identity: bool,
    pub entry_count: i64,
}

/// Turn a verified working copy in `tmp_dir` (inside `parent`) into a registered-ready live
/// logbook: migrate, optionally assign a new identity, record provenance, verify attachments,
/// then rename into its final folder. Cleans up `tmp_dir` on failure.
pub fn finalize_install(
    tmp_dir: &Path,
    parent: &Path,
    name_override: Option<&str>,
    new_identity: bool,
    provenance_action: serde_json::Value,
) -> AppResult<InstallOutcome> {
    let result = (|| -> AppResult<InstallOutcome> {
        let mut conn = Connection::open(tmp_dir.join(DB_FILE))?;
        logbook::configure_connection(&conn, false)?;
        logbook::integrity_check(&conn)?;
        migrate::migrate(&mut conn, LOGBOOK_MIGRATIONS)?;
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM logbook", [], |r| r.get(0))?;
        if n != 1 {
            return Err(AppError::new(ErrorCode::InvalidPackage, "Database must contain exactly one logbook"));
        }
        let (source_id, src_name, ty, created, prov): (String, String, String, String, String) =
            conn.query_row("SELECT id, name, type, created_at_utc, provenance_json FROM logbook", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?;
        let logbook_type = LogbookType::parse(&ty).ok_or_else(|| AppError::new(ErrorCode::InvalidPackage, "Unknown logbook type"))?;
        let name = logbook::validate_name(name_override.unwrap_or(&src_name))?;
        let safe = fsutil::safe_component(&name, 60);
        let prefix = format!("{safe}--");
        for e in fs::read_dir(parent)?.flatten() {
            if e.file_name().to_string_lossy().starts_with(&prefix) && e.path().join(DESCRIPTOR_FILE).exists() {
                return Err(AppError::new(
                    ErrorCode::AlreadyExists,
                    format!("A logbook named '{name}' already exists in this folder. Choose a different name or folder."),
                ));
            }
        }
        let id = if new_identity { fsutil::new_uuid() } else { source_id.clone() };
        let mut provenance: serde_json::Value = serde_json::from_str(&prov).unwrap_or(serde_json::json!([]));
        let mut action = provenance_action;
        if let Some(o) = action.as_object_mut() {
            o.insert("source_uuid".into(), serde_json::json!(source_id));
            o.insert("source_name".into(), serde_json::json!(src_name));
            o.insert("at_utc".into(), serde_json::json!(now_utc()));
            o.insert("new_identity".into(), serde_json::json!(new_identity));
        }
        if let Some(arr) = provenance.as_array_mut() {
            arr.push(action);
        } else {
            provenance = serde_json::json!([action]);
        }
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE logbook SET id=?1, name=?2, provenance_json=?3, updated_at_utc=?4",
            params![id, name, provenance.to_string(), now_utc()],
        )?;
        tx.commit()?;
        {
            let mut stmt = conn.prepare("SELECT rel_path, sha256, size_bytes FROM attachments")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))?;
            for r in rows {
                let (rel, sha, size) = r?;
                if !fsutil::is_plain_relative(&rel) || !rel.starts_with(&format!("{ATTACHMENTS_DIR}/")) {
                    return Err(AppError::new(ErrorCode::InvalidPackage, format!("Unsafe attachment path: {rel}")));
                }
                let (h, n) = fsutil::sha256_file(&tmp_dir.join(&rel))
                    .map_err(|_| AppError::new(ErrorCode::Integrity, format!("Attachment missing: {rel}")))?;
                if h != sha || n as i64 != size {
                    return Err(AppError::new(ErrorCode::Integrity, format!("Attachment does not match its hash: {rel}")));
                }
            }
        }
        logbook::integrity_check(&conn)?;
        let entry_count: i64 = conn.query_row("SELECT COUNT(*) FROM entries", [], |r| r.get(0))?;
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        conn.close().map_err(|(_, e)| AppError::from(e))?;
        fs::create_dir_all(tmp_dir.join(ATTACHMENTS_DIR))?;
        logbook::write_descriptor(
            tmp_dir,
            &Descriptor {
                format: "EMCOMM-LOG-LIVE".into(),
                descriptor_version: 1,
                logbook_id: id.clone(),
                name: name.clone(),
                logbook_type,
                schema_version: migrate::latest_version(LOGBOOK_MIGRATIONS),
                created_at_utc: created,
                app_version: logbook::APP_VERSION.into(),
            },
        )?;
        fsutil::sync_dir(tmp_dir)?;
        let final_dir = parent.join(format!("{safe}--{}", short_uuid(&id)));
        if final_dir.exists() {
            return Err(AppError::new(ErrorCode::AlreadyExists, "Target folder already exists"));
        }
        fs::rename(tmp_dir, &final_dir)?;
        fsutil::sync_dir(parent)?;
        Ok(InstallOutcome { path: final_dir, logbook_id: id, source_uuid: source_id, new_identity, entry_count })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(tmp_dir);
    }
    result
}

/// Restore a verified backup as a separate logbook under `parent`. Never touches existing books.
/// If `id_in_use` reports the original UUID is already known locally, a new identity is assigned.
pub fn restore_backup(
    backup_dir: &Path,
    parent: &Path,
    name_override: Option<&str>,
    id_in_use: &dyn Fn(&str) -> bool,
) -> AppResult<InstallOutcome> {
    let m = snapshot::verify_dir(backup_dir, BACKUP_FORMAT)?;
    fsutil::check_writable(parent)?;
    logbook::check_live_location(parent)?;
    if fsutil::is_within(parent, backup_dir) {
        return Err(AppError::validation("parent_dir", "Restore into a folder outside the backup"));
    }
    let tmp = parent.join(format!(".emcomm-restoring-{}", short_uuid(&fsutil::new_uuid())));
    let copy = (|| -> AppResult<()> {
        fs::create_dir(&tmp)?;
        fs::create_dir(tmp.join(ATTACHMENTS_DIR))?;
        for f in &m.files {
            let (h, n) = fsutil::copy_synced(&backup_dir.join(&f.path), &tmp.join(&f.path))?;
            if h != f.sha256 || n != f.bytes {
                return Err(AppError::new(ErrorCode::Integrity, format!("Copy of {} did not verify", f.path)));
            }
        }
        Ok(())
    })();
    if let Err(e) = copy {
        let _ = fs::remove_dir_all(&tmp);
        return Err(e);
    }
    let new_identity = id_in_use(&m.source.logbook_id);
    finalize_install(
        &tmp,
        parent,
        name_override,
        new_identity,
        serde_json::json!({"action": "restore", "backup_created_at_utc": m.created_at_utc, "backup_kind": m.backup_kind}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn retention_keeps_recent_daily_weekly() {
        let s = BackupSettings { keep_recent: 2, keep_daily: 3, keep_weekly: 2, ..Default::default() };
        let mut items = Vec::new();
        // 10 days, 4 backups per day
        for d in 0..10 {
            for h in [1, 7, 13, 19] {
                let t = Utc.with_ymd_and_hms(2026, 10, 20 + d, h, 0, 0).unwrap();
                items.push((t, PathBuf::from(format!("{d}-{h}"))));
            }
        }
        let keep = retention_keep(items, &s);
        assert!(keep.contains(&PathBuf::from("9-19")));
        assert!(keep.contains(&PathBuf::from("9-13")));
        assert!(keep.contains(&PathBuf::from("8-19")));
        assert!(keep.contains(&PathBuf::from("7-19")));
        assert!(!keep.contains(&PathBuf::from("9-1")));
        // Previous ISO week's newest (Oct 25, 2026 is a Sunday) is kept as a weekly.
        assert!(keep.contains(&PathBuf::from("5-19")));
        assert!(keep.len() <= 2 + 3 + 2);
    }
}
