//! Consistent point-in-time copies of a live logbook (database + referenced attachments),
//! shared by backups and `.emlog` packages.

use crate::error::{AppError, AppResult, ErrorCode};
use crate::fsutil::{self, now_utc};
use crate::logbook::{self, Logbook, ATTACHMENTS_DIR, DB_FILE};
use crate::models::LogbookType;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManifestFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManifestSource {
    pub logbook_id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub logbook_type: LogbookType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub format: String,
    pub format_version: u32,
    pub schema_version: u32,
    pub created_at_utc: String,
    pub app_version: String,
    pub source: ManifestSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_kind: Option<String>,
    pub entry_count: i64,
    pub max_sequence: i64,
    #[serde(default)]
    pub provenance: serde_json::Value,
    pub files: Vec<ManifestFile>,
}

pub const PACKAGE_FORMAT: &str = "EMCOMM-LOG";
pub const BACKUP_FORMAT: &str = "EMCOMM-LOG-BACKUP";
pub const FORMAT_VERSION: u32 = 1;
pub const MANIFEST_FILE: &str = "manifest.json";

pub struct SnapshotInfo {
    pub logbook_id: String,
    pub name: String,
    pub logbook_type: LogbookType,
    pub schema_version: u32,
    pub entry_count: i64,
    pub max_sequence: i64,
    pub provenance: serde_json::Value,
    pub data_marker: String,
    pub files: Vec<ManifestFile>,
}

/// A digest of everything that changes when the logbook content changes. Used to skip
/// automatic backups when nothing happened since the last successful one.
pub fn data_marker(conn: &Connection) -> AppResult<String> {
    let parts: (String, i64, i64, i64, i64, Option<String>, String, String) = conn.query_row(
        "SELECT (SELECT updated_at_utc FROM logbook),
                (SELECT COUNT(*) FROM entry_revisions),
                (SELECT COUNT(*) FROM attachments),
                (SELECT COUNT(*) FROM entry_favorites),
                (SELECT COUNT(*) FROM entry_templates),
                (SELECT MAX(updated_at_utc) FROM entry_templates),
                (SELECT updated_at_utc FROM logbook_settings),
                (SELECT COALESCE(group_concat(entry_id), '') FROM (SELECT entry_id FROM entry_favorites ORDER BY entry_id))",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)),
    )?;
    Ok(fsutil::sha256_bytes(format!("{parts:?}").as_bytes()))
}

pub fn live_data_marker(live_dir: &Path) -> AppResult<String> {
    let conn = Logbook::open_read_only(&live_dir.join(DB_FILE))?;
    data_marker(&conn)
}

/// Write a consistent copy of the live database and its referenced attachments into `out_dir`.
/// The copy comes from one read transaction (`VACUUM INTO`), so concurrent saves are either
/// wholly in it or wholly absent. Attachment files are content-addressed and immutable, and are
/// copied based on the snapshot's own attachment rows, then hash-verified.
pub fn take_snapshot(live_dir: &Path, out_dir: &Path) -> AppResult<SnapshotInfo> {
    let live_db = live_dir.join(DB_FILE);
    if !live_db.is_file() {
        return Err(AppError::new(ErrorCode::StorageUnavailable, "The live logbook database is not available"));
    }
    fs::create_dir_all(out_dir.join(ATTACHMENTS_DIR))?;
    let snap_db = out_dir.join(DB_FILE);
    {
        let src = Connection::open_with_flags(&live_db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        src.busy_timeout(std::time::Duration::from_secs(10))?;
        src.execute("VACUUM INTO ?1", params![snap_db.to_string_lossy()])?;
    }
    crate::faults::check("snapshot.after_db")?;
    let conn = Connection::open(&snap_db)?;
    conn.query_row("PRAGMA journal_mode=DELETE", [], |_| Ok(()))?;
    logbook::integrity_check(&conn)?;
    let (logbook_id, name, ty, schema_version, provenance): (String, String, String, u32, String) = conn.query_row(
        "SELECT id, name, type, schema_version, provenance_json FROM logbook",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    let (entry_count, max_sequence): (i64, i64) =
        conn.query_row("SELECT COUNT(*), COALESCE(MAX(sequence_no), 0) FROM entries", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let marker = data_marker(&conn)?;
    let mut atts: BTreeMap<String, (String, u64)> = BTreeMap::new();
    {
        let mut stmt = conn.prepare("SELECT rel_path, sha256, size_bytes FROM attachments")?;
        for r in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))? {
            let (p, h, s) = r?;
            atts.insert(p, (h, s as u64));
        }
    }
    drop(conn);
    let mut files = Vec::new();
    for (rel, (sha, size)) in &atts {
        if !fsutil::is_plain_relative(rel) || !rel.starts_with(&format!("{ATTACHMENTS_DIR}/")) {
            return Err(AppError::new(ErrorCode::Integrity, format!("Attachment path '{rel}' is invalid")));
        }
        let src = live_dir.join(rel);
        let dst = out_dir.join(rel);
        let (h, n) = fsutil::copy_synced(&src, &dst)
            .map_err(|e| e.with_context(&format!("Attachment {rel} could not be copied")))?;
        if &h != sha || n != *size {
            return Err(AppError::new(ErrorCode::Integrity, format!("Attachment {rel} does not match its recorded hash")));
        }
        files.push(ManifestFile { path: rel.clone(), bytes: n, sha256: h });
    }
    let (db_hash, db_size) = fsutil::sha256_file(&snap_db)?;
    files.insert(0, ManifestFile { path: DB_FILE.into(), bytes: db_size, sha256: db_hash });
    fsutil::sync_dir(&out_dir.join(ATTACHMENTS_DIR))?;
    fsutil::sync_dir(out_dir)?;
    Ok(SnapshotInfo {
        logbook_id,
        name,
        logbook_type: LogbookType::parse(&ty).unwrap_or(LogbookType::General),
        schema_version,
        entry_count,
        max_sequence,
        provenance: serde_json::from_str(&provenance).unwrap_or(serde_json::json!([])),
        data_marker: marker,
        files,
    })
}

pub fn manifest_for(snap: &SnapshotInfo, format: &str, backup_kind: Option<&str>) -> Manifest {
    Manifest {
        format: format.into(),
        format_version: FORMAT_VERSION,
        schema_version: snap.schema_version,
        created_at_utc: now_utc(),
        app_version: logbook::APP_VERSION.into(),
        source: ManifestSource { logbook_id: snap.logbook_id.clone(), name: snap.name.clone(), logbook_type: snap.logbook_type },
        backup_kind: backup_kind.map(String::from),
        entry_count: snap.entry_count,
        max_sequence: snap.max_sequence,
        provenance: snap.provenance.clone(),
        files: snap.files.clone(),
    }
}

/// Verify a directory-form snapshot (backup) against its manifest: every listed file present with
/// matching size and hash, no unlisted files, and a structurally sound database.
pub fn verify_dir(dir: &Path, expected_format: &str) -> AppResult<Manifest> {
    let raw = fs::read(dir.join(MANIFEST_FILE)).map_err(|_| AppError::new(ErrorCode::InvalidPackage, "Backup manifest is missing"))?;
    let m: Manifest = serde_json::from_slice(&raw).map_err(|_| AppError::new(ErrorCode::InvalidPackage, "Backup manifest is malformed"))?;
    if m.format != expected_format {
        return Err(AppError::new(ErrorCode::InvalidPackage, "This folder is not an EMCOMM LOG backup"));
    }
    if m.format_version > FORMAT_VERSION || m.schema_version > crate::migrate::latest_version(crate::migrate::LOGBOOK_MIGRATIONS) {
        return Err(AppError::new(ErrorCode::UnsupportedVersion, "This backup was created by a newer version of EMCOMM LOG"));
    }
    let mut listed = std::collections::HashSet::new();
    for f in &m.files {
        if !fsutil::is_plain_relative(&f.path) {
            return Err(AppError::new(ErrorCode::InvalidPackage, format!("Unsafe path in manifest: {}", f.path)));
        }
        let p = dir.join(&f.path);
        let (h, n) = fsutil::sha256_file(&p).map_err(|_| AppError::new(ErrorCode::Integrity, format!("Backup file missing: {}", f.path)))?;
        if h != f.sha256 || n != f.bytes {
            return Err(AppError::new(ErrorCode::Integrity, format!("Backup file is corrupt: {}", f.path)));
        }
        listed.insert(f.path.clone());
    }
    if !listed.contains(DB_FILE) {
        return Err(AppError::new(ErrorCode::InvalidPackage, "Backup has no database"));
    }
    if let Ok(rd) = fs::read_dir(dir.join(ATTACHMENTS_DIR)) {
        for e in rd.flatten() {
            let rel = format!("{ATTACHMENTS_DIR}/{}", e.file_name().to_string_lossy());
            if !listed.contains(&rel) {
                return Err(AppError::new(ErrorCode::Integrity, format!("Unlisted file in backup: {rel}")));
            }
        }
    }
    let conn = Logbook::open_read_only(&dir.join(DB_FILE))?;
    logbook::integrity_check(&conn)?;
    let (id, count): (String, i64) =
        conn.query_row("SELECT (SELECT id FROM logbook), (SELECT COUNT(*) FROM entries)", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    if id != m.source.logbook_id || count != m.entry_count {
        return Err(AppError::new(ErrorCode::Integrity, "Backup database does not match its manifest"));
    }
    Ok(m)
}
