use crate::error::{AppError, AppResult, ErrorCode};
use crate::fsutil::{self, now_utc, short_uuid};
use crate::migrate::{self, LOGBOOK_MIGRATIONS};
use crate::models::*;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DB_FILE: &str = "logbook.sqlite";
pub const DESCRIPTOR_FILE: &str = "logbook.json";
pub const ATTACHMENTS_DIR: &str = "attachments";
pub const STAGING_DIR: &str = ".staging";
pub const LOCK_FILE: &str = ".emcomm.lock";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const MAX_ATTACHMENT_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Descriptor {
    pub format: String,
    pub descriptor_version: u32,
    pub logbook_id: String,
    pub name: String,
    pub logbook_type: LogbookType,
    pub schema_version: u32,
    pub created_at_utc: String,
    pub app_version: String,
}

/// Apply and verify the durability pragmas required by LOG-STORAGE-001.
pub fn configure_connection(conn: &Connection, read_only: bool) -> AppResult<()> {
    conn.busy_timeout(Duration::from_secs(5))?;
    if read_only {
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA query_only=ON;")?;
        return Ok(());
    }
    let mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(AppError::new(
            ErrorCode::StorageUnavailable,
            format!("Could not enable WAL journaling on this storage (got '{mode}')"),
        ));
    }
    conn.execute_batch("PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;")?;
    #[cfg(target_os = "macos")]
    conn.execute_batch("PRAGMA fullfsync=ON; PRAGMA checkpoint_fullfsync=ON;")?;
    let sync: i64 = conn.query_row("PRAGMA synchronous", [], |r| r.get(0))?;
    let fk: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
    if sync != 2 || fk != 1 {
        return Err(AppError::new(
            ErrorCode::Integrity,
            format!("Durability settings not in effect (synchronous={sync}, foreign_keys={fk})"),
        ));
    }
    Ok(())
}

pub fn effective_pragmas(conn: &Connection) -> AppResult<(String, i64, i64)> {
    let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
    let sync: i64 = conn.query_row("PRAGMA synchronous", [], |r| r.get(0))?;
    let fk: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
    Ok((mode, sync, fk))
}

pub const PRE_MIGRATION_DIR: &str = ".pre-migration";

/// Copy the database (as it is before a schema upgrade) into `<logbook>/.pre-migration/`, verified,
/// so an upgrade can always be undone by hand. Snapshots, backups and packages never include it.
fn pre_migration_copy(conn: &Connection, dir: &Path, from: u32) -> AppResult<PathBuf> {
    let out_dir = dir.join(PRE_MIGRATION_DIR);
    fs::create_dir_all(&out_dir)?;
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let path = out_dir.join(format!("logbook-schema{from}-{stamp}.sqlite"));
    conn.execute("VACUUM INTO ?1", params![path.to_string_lossy()])
        .map_err(|e| AppError::from(e).with_context("Could not save a copy of the logbook before upgrading it; nothing was changed"))?;
    fsutil::sync_file(&path)?;
    fsutil::sync_dir(&out_dir)?;
    let check = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    integrity_check(&check).map_err(|e| e.with_context("The pre-upgrade copy failed its check; nothing was changed"))?;
    Ok(path)
}

pub fn integrity_check(conn: &Connection) -> AppResult<()> {
    let res: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if res != "ok" {
        return Err(AppError::new(ErrorCode::Integrity, format!("Integrity check failed: {res}")));
    }
    let mut stmt = conn.prepare("PRAGMA foreign_key_check")?;
    let mut rows = stmt.query([])?;
    if rows.next()?.is_some() {
        return Err(AppError::new(ErrorCode::Integrity, "Foreign key check failed"));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    dev: u64,
    ino: u64,
}

fn file_identity(p: &Path) -> Option<FileIdentity> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        fs::metadata(p).ok().map(|m| FileIdentity { dev: m.dev(), ino: m.ino() })
    }
    #[cfg(not(unix))]
    {
        fs::metadata(p).ok().map(|_| FileIdentity { dev: 0, ino: 0 })
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct LockInfo {
    pid: u32,
    opened_at_utc: String,
    app_version: String,
}

pub struct LockGuard {
    file: File,
}

impl LockGuard {
    /// Take an exclusive OS lock. Returns (guard, previous_owner_crashed).
    fn acquire(dir: &Path) -> AppResult<(LockGuard, bool)> {
        let path = dir.join(LOCK_FILE);
        let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => {
                return Err(AppError::new(ErrorCode::Locked, "This logbook is already open in another EMCOMM LOG window or process"));
            }
            Err(fs::TryLockError::Error(e)) => return Err(e.into()),
        }
        let stale = file.metadata().map(|m| m.len() > 0).unwrap_or(false);
        let mut g = LockGuard { file };
        g.write_info()?;
        Ok((g, stale))
    }

    fn write_info(&mut self) -> AppResult<()> {
        let info = LockInfo { pid: std::process::id(), opened_at_utc: now_utc(), app_version: APP_VERSION.into() };
        self.file.set_len(0)?;
        let mut f = &self.file;
        use std::io::Seek;
        f.seek(std::io::SeekFrom::Start(0))?;
        f.write_all(serde_json::to_string(&info)?.as_bytes())?;
        f.sync_all()?;
        Ok(())
    }

    fn release_clean(&self) {
        let _ = self.file.set_len(0);
        let _ = self.file.sync_all();
        let _ = self.file.unlock();
    }
}

pub struct Logbook {
    pub conn: Connection,
    pub dir: PathBuf,
    pub id: String,
    pub logbook_type: LogbookType,
    pub read_only: bool,
    lock: Option<LockGuard>,
    db_identity: Option<FileIdentity>,
    pub notices: Vec<String>,
    /// An archived logbook made by an older version is shown from an upgraded temporary copy, so the
    /// archived file itself is never changed (2.1 spec §8.2). Unarchiving upgrades it in place.
    view_copy: Option<PathBuf>,
}

pub struct CreateParams {
    pub logbook_type: LogbookType,
    pub name: String,
    pub parent_dir: PathBuf,
    pub incident: IncidentFields,
    pub profile: OperatorSnapshot,
    pub preset: Option<Preset>,
}

pub fn validate_name(name: &str) -> AppResult<String> {
    let n = name.trim();
    if n.is_empty() {
        return Err(AppError::validation("name", "Logbook name is required"));
    }
    if n.chars().count() > 120 || n.chars().any(|c| c.is_control()) {
        return Err(AppError::validation("name", "Name must be 1-120 characters without control characters"));
    }
    Ok(n.to_string())
}

/// Reject storage locations SQLite cannot safely share or that sync behind our back.
pub fn check_live_location(dir: &Path) -> AppResult<()> {
    match fsutil::classify_location(dir) {
        fsutil::LocationKind::Local => Ok(()),
        fsutil::LocationKind::Network => Err(AppError::new(
            ErrorCode::UnsupportedLocation,
            "Network folders are not supported for live logbooks. Choose a local disk or directly attached drive.",
        )),
        fsutil::LocationKind::CloudSync => Err(AppError::new(
            ErrorCode::UnsupportedLocation,
            "Cloud-synchronized folders (iCloud, Dropbox, OneDrive, Google Drive) can corrupt a live database. Choose a local folder; use backups or .emlog packages for copies.",
        )),
    }
}

fn find_name_collision(parent: &Path, safe: &str, name: &str) -> AppResult<()> {
    let prefix = format!("{safe}--");
    if let Ok(rd) = fs::read_dir(parent) {
        for e in rd.flatten() {
            let fname = e.file_name().to_string_lossy().to_string();
            if fname.starts_with(&prefix) && e.path().join(DESCRIPTOR_FILE).exists() {
                return Err(AppError::new(
                    ErrorCode::AlreadyExists,
                    format!("A logbook named '{name}' already exists in this folder. Choose a different name or folder."),
                ));
            }
        }
    }
    Ok(())
}

pub fn write_descriptor(dir: &Path, d: &Descriptor) -> AppResult<()> {
    fsutil::atomic_write(&dir.join(DESCRIPTOR_FILE), serde_json::to_string_pretty(d)?.as_bytes())
}

pub fn read_descriptor(dir: &Path) -> Option<Descriptor> {
    fs::read(dir.join(DESCRIPTOR_FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

/// Insert the logbook identity, settings snapshot and preset templates into a freshly migrated DB.
pub(crate) fn seed_new_logbook(conn: &mut Connection, id: &str, p: &CreateParams, name: &str) -> AppResult<()> {
    let now = now_utc();
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let inc = &p.incident;
    tx.execute(
        "INSERT INTO logbook(id, schema_version, type, name, incident_name, incident_number, operational_period, agency, location,
            created_at_utc, updated_at_utc, next_sequence, origin_profile_snapshot_json, provenance_json, created_by_app_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10, 1, ?11, '[]', ?12)",
        params![
            id,
            migrate::latest_version(LOGBOOK_MIGRATIONS),
            p.logbook_type.as_str(),
            name,
            inc.incident_name.trim(),
            inc.incident_number.trim(),
            inc.operational_period.trim(),
            inc.agency.trim(),
            inc.location.trim(),
            now,
            serde_json::to_string(&p.profile)?,
            APP_VERSION
        ],
    )?;
    let (preset_id, preset_name, preset_version, config) = match &p.preset {
        Some(pr) => (Some(pr.id.clone()), pr.name.clone(), pr.version, pr.config.clone()),
        None => (None, String::new(), 0, PresetConfig::default()),
    };
    let mut snapshot = config.clone();
    snapshot.templates.clear();
    tx.execute(
        "INSERT INTO logbook_settings(logbook_id, preset_id, preset_name, preset_version, preset_snapshot_json, updated_at_utc)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, preset_id, preset_name, preset_version, serde_json::to_string(&snapshot)?, now],
    )?;
    for t in &config.templates {
        tx.execute(
            "INSERT INTO entry_templates(id, logbook_id, name, kind, version, defaults_json, created_at_utc, updated_at_utc)
             VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?6)",
            params![
                fsutil::new_uuid(),
                id,
                t.name,
                t.defaults.kind.map(|k| k.as_str()).unwrap_or(""),
                serde_json::to_string(&t.defaults)?,
                now
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

impl Logbook {
    /// Create a new live logbook directory. Built in a hidden temp directory and renamed into
    /// place so a failure never leaves a half-created logbook that looks valid.
    pub fn create(p: CreateParams) -> AppResult<PathBuf> {
        let name = validate_name(&p.name)?;
        if p.logbook_type == LogbookType::Emergency && p.incident.incident_name.trim().is_empty() {
            return Err(AppError::validation("incident_name", "Emergency logbooks need an incident name"));
        }
        let parent = &p.parent_dir;
        fsutil::check_writable(parent)?;
        check_live_location(parent)?;
        if parent.join(DB_FILE).exists() || parent.join(DESCRIPTOR_FILE).exists() {
            return Err(AppError::validation("parent_dir", "Choose a folder that is not itself a logbook"));
        }
        let safe = fsutil::safe_component(&name, 60);
        find_name_collision(parent, &safe, &name)?;
        let id = fsutil::new_uuid();
        let final_dir = parent.join(format!("{safe}--{}", short_uuid(&id)));
        let tmp_dir = parent.join(format!(".emcomm-creating-{}", short_uuid(&id)));
        let result = (|| -> AppResult<()> {
            fs::create_dir(&tmp_dir)?;
            fs::create_dir(tmp_dir.join(ATTACHMENTS_DIR))?;
            {
                let mut conn = Connection::open(tmp_dir.join(DB_FILE))?;
                configure_connection(&conn, false)?;
                migrate::migrate(&mut conn, LOGBOOK_MIGRATIONS)?;
                seed_new_logbook(&mut conn, &id, &p, &name)?;
                conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
                conn.close().map_err(|(_, e)| AppError::from(e))?;
            }
            write_descriptor(
                &tmp_dir,
                &Descriptor {
                    format: "EMCOMM-LOG-LIVE".into(),
                    descriptor_version: 1,
                    logbook_id: id.clone(),
                    name: name.clone(),
                    logbook_type: p.logbook_type,
                    schema_version: migrate::latest_version(LOGBOOK_MIGRATIONS),
                    created_at_utc: now_utc(),
                    app_version: APP_VERSION.into(),
                },
            )?;
            fsutil::sync_dir(&tmp_dir)?;
            if final_dir.exists() {
                return Err(AppError::new(ErrorCode::AlreadyExists, "Target logbook folder already exists"));
            }
            fs::rename(&tmp_dir, &final_dir)?;
            fsutil::sync_dir(parent)?;
            Ok(())
        })();
        if let Err(e) = result {
            let _ = fs::remove_dir_all(&tmp_dir);
            return Err(e);
        }
        Ok(final_dir)
    }

    /// Open a live logbook for editing. Never creates a database: a missing file is reported.
    pub fn open(dir: &Path) -> AppResult<Logbook> {
        let db_path = dir.join(DB_FILE);
        if !dir.exists() {
            return Err(AppError::new(
                ErrorCode::StorageUnavailable,
                format!("Logbook folder not found: {} (drive disconnected or folder moved?)", dir.display()),
            ));
        }
        if !db_path.exists() {
            return Err(AppError::new(ErrorCode::StorageUnavailable, "logbook.sqlite is missing from this folder"));
        }
        check_live_location(dir)?;
        let (lock, stale) = LockGuard::acquire(dir)?;
        let mut conn = Connection::open_with_flags(
            &db_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI,
        )?;
        configure_connection(&conn, false)?;
        let mut notices = Vec::new();
        if stale {
            integrity_check(&conn).map_err(|e| e.with_context("Recovered from an unclean shutdown but the database failed checks"))?;
            notices.push("The logbook was not closed cleanly last time. Integrity check passed.".to_string());
        }
        let from = migrate::current_version(&conn)?;
        let to = migrate::latest_version(LOGBOOK_MIGRATIONS);
        let archived_now: bool = conn.query_row("SELECT archived_at_utc IS NOT NULL FROM logbook", [], |r| r.get(0)).unwrap_or(false);
        let mut view_copy = None;
        if from > 0 && from < to && archived_now {
            let tmp = std::env::temp_dir().join(format!("emcomm-archived-view-{}.sqlite", fsutil::new_uuid()));
            conn.execute("VACUUM INTO ?1", params![tmp.to_string_lossy()])?;
            conn = Connection::open(&tmp)?;
            configure_connection(&conn, false)?;
            view_copy = Some(tmp);
            notices.push(format!(
                "This archived logbook was made by an older version (schema {from}). It is shown from an upgraded temporary copy; the archived file is not changed. Unarchive it to upgrade it in place."
            ));
        } else if from > 0 && from < to {
            let copy = pre_migration_copy(&conn, dir, from)?;
            notices.push(format!(
                "This logbook was upgraded from schema {from} to {to} for this version of EMCOMM LOG. A copy of the previous version was kept at {}.",
                copy.display()
            ));
        }
        migrate::migrate(&mut conn, LOGBOOK_MIGRATIONS)?;
        let (id, ty, archived, name, created): (String, String, Option<String>, String, String) = conn
            .query_row("SELECT id, type, archived_at_utc, name, created_at_utc FROM logbook", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .map_err(|_| AppError::new(ErrorCode::Integrity, "This database has no logbook identity row"))?;
        let logbook_type = LogbookType::parse(&ty).ok_or_else(|| AppError::new(ErrorCode::Integrity, "Unknown logbook type"))?;
        let expected = Descriptor {
            format: "EMCOMM-LOG-LIVE".into(),
            descriptor_version: 1,
            logbook_id: id.clone(),
            name: name.clone(),
            logbook_type,
            schema_version: migrate::latest_version(LOGBOOK_MIGRATIONS),
            created_at_utc: created,
            app_version: APP_VERSION.into(),
        };
        match read_descriptor(dir) {
            _ if view_copy.is_some() => {}
            Some(d)
                if d.logbook_id == expected.logbook_id
                    && d.name == expected.name
                    && d.logbook_type == expected.logbook_type
                    && d.schema_version == expected.schema_version => {}
            Some(d) => {
                if d.logbook_id != id {
                    notices.push("logbook.json did not match the database identity and was rewritten from the database.".into());
                }
                write_descriptor(dir, &expected)?;
            }
            None => {
                notices.push("logbook.json was missing and has been recreated from the database.".into());
                write_descriptor(dir, &expected)?;
            }
        }
        fs::create_dir_all(dir.join(ATTACHMENTS_DIR))?;
        let db_identity = file_identity(&db_path);
        Ok(Logbook {
            conn,
            dir: dir.to_path_buf(),
            id,
            logbook_type,
            read_only: archived.is_some(),
            lock: Some(lock),
            db_identity,
            notices,
            view_copy,
        })
    }

    /// Test-only: a second writer connection to the same file without the app lock, to prove
    /// SQLite-level serialization of sequence allocation under truly concurrent writers.
    #[cfg(feature = "fault-injection")]
    pub fn open_unlocked_for_test(dir: &Path) -> AppResult<Logbook> {
        let mut conn = Connection::open(dir.join(DB_FILE))?;
        configure_connection(&conn, false)?;
        migrate::migrate(&mut conn, LOGBOOK_MIGRATIONS)?;
        let (id, ty): (String, String) = conn.query_row("SELECT id, type FROM logbook", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(Logbook {
            conn,
            dir: dir.to_path_buf(),
            id,
            logbook_type: LogbookType::parse(&ty).unwrap(),
            read_only: false,
            lock: None,
            db_identity: file_identity(&dir.join(DB_FILE)),
            notices: vec![],
            view_copy: None,
        })
    }

    /// Open a database read-only without taking the edit lock (previews, validation).
    pub fn open_read_only(db_path: &Path) -> AppResult<Connection> {
        let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        configure_connection(&conn, true)?;
        Ok(conn)
    }

    pub fn db_path(&self) -> PathBuf {
        self.dir.join(DB_FILE)
    }

    /// Confirm the live location is still the one we opened. Run before every final write.
    pub fn check_storage(&self) -> AppResult<()> {
        let db = self.db_path();
        if !self.dir.is_dir() || !db.is_file() {
            return Err(AppError::new(
                ErrorCode::StorageUnavailable,
                "The logbook location is unavailable (drive disconnected or folder moved). Final saves are blocked until it is reconnected.",
            ));
        }
        if let (Some(a), Some(b)) = (self.db_identity, file_identity(&db))
            && a != b
        {
            return Err(AppError::new(
                ErrorCode::StorageUnavailable,
                "The logbook file at the live path was replaced or moved. Final saves are blocked; reopen the logbook.",
            ));
        }
        if !self.dir.join(ATTACHMENTS_DIR).is_dir() {
            return Err(AppError::new(ErrorCode::StorageUnavailable, "The logbook attachments folder is missing"));
        }
        Ok(())
    }

    pub fn ensure_writable(&self) -> AppResult<()> {
        if self.read_only {
            return Err(AppError::new(ErrorCode::ReadOnly, "This logbook is archived. Unarchive it to make changes."));
        }
        self.check_storage()
    }

    pub fn health(&self) -> AppResult<StorageHealth> {
        let storage = self.check_storage();
        let (journal_mode, synchronous, foreign_keys) = effective_pragmas(&self.conn)?;
        let probe = if storage.is_ok() { fsutil::check_writable(&self.dir).err() } else { None };
        let err = storage.err().or(probe);
        Ok(StorageHealth {
            ok: err.is_none() && journal_mode.eq_ignore_ascii_case("wal") && synchronous == 2,
            path: self.dir.to_string_lossy().to_string(),
            journal_mode,
            synchronous,
            foreign_keys: foreign_keys == 1,
            error: err,
        })
    }

    pub fn info(&self) -> AppResult<LogbookInfo> {
        let row = self.conn.query_row(
            "SELECT id, type, name, incident_name, incident_number, operational_period, agency, location, created_at_utc,
                    updated_at_utc, archived_at_utc, next_sequence, schema_version, origin_profile_snapshot_json, provenance_json
             FROM logbook",
            [],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    IncidentFields {
                        incident_name: r.get(3)?,
                        incident_number: r.get(4)?,
                        operational_period: r.get(5)?,
                        agency: r.get(6)?,
                        location: r.get(7)?,
                    },
                    r.get::<_, String>(8)?,
                    r.get::<_, String>(9)?,
                    r.get::<_, Option<String>>(10)?,
                    r.get::<_, i64>(11)?,
                    r.get::<_, u32>(12)?,
                    r.get::<_, String>(13)?,
                    r.get::<_, String>(14)?,
                ))
            },
        )?;
        Ok(LogbookInfo {
            id: row.0,
            logbook_type: LogbookType::parse(&row.1).unwrap_or(LogbookType::General),
            name: row.2,
            incident: row.3,
            created_at_utc: row.4,
            updated_at_utc: row.5,
            archived_at_utc: row.6,
            next_sequence: row.7,
            schema_version: row.8,
            origin_profile_snapshot: serde_json::from_str(&row.9).unwrap_or_default(),
            provenance: serde_json::from_str(&row.10).unwrap_or(serde_json::json!([])),
            path: self.dir.to_string_lossy().to_string(),
            read_only: self.read_only,
        })
    }

    pub fn settings(&self) -> AppResult<LogbookSettings> {
        read_settings(&self.conn)
    }

    pub fn update_settings(&mut self, time_display_override: Option<String>, operational_notes: String) -> AppResult<LogbookSettings> {
        self.ensure_writable()?;
        if let Some(t) = &time_display_override
            && t != "local"
            && t != "utc"
        {
            return Err(AppError::validation("time_display_override", "Use local or utc"));
        }
        if operational_notes.chars().count() > 10_000 {
            return Err(AppError::validation("operational_notes", "Notes must be at most 10,000 characters"));
        }
        let tx = self.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE logbook_settings SET time_display_override=?1, operational_notes=?2, updated_at_utc=?3",
            params![time_display_override, operational_notes, now_utc()],
        )?;
        tx.commit()?;
        self.settings()
    }

    pub fn update_identity(&mut self, name: &str, incident: &IncidentFields) -> AppResult<LogbookInfo> {
        self.ensure_writable()?;
        let name = validate_name(name)?;
        for (f, v) in [
            ("incident_name", &incident.incident_name),
            ("incident_number", &incident.incident_number),
            ("operational_period", &incident.operational_period),
            ("agency", &incident.agency),
            ("location", &incident.location),
        ] {
            if v.chars().count() > 200 {
                return Err(AppError::validation(f, "Must be at most 200 characters"));
            }
        }
        if self.logbook_type == LogbookType::Emergency && incident.incident_name.trim().is_empty() {
            return Err(AppError::validation("incident_name", "Emergency logbooks need an incident name"));
        }
        let tx = self.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE logbook SET name=?1, incident_name=?2, incident_number=?3, operational_period=?4, agency=?5, location=?6, updated_at_utc=?7",
            params![
                name,
                incident.incident_name.trim(),
                incident.incident_number.trim(),
                incident.operational_period.trim(),
                incident.agency.trim(),
                incident.location.trim(),
                now_utc()
            ],
        )?;
        tx.commit()?;
        let mut d = read_descriptor(&self.dir).unwrap_or(Descriptor {
            format: "EMCOMM-LOG-LIVE".into(),
            descriptor_version: 1,
            logbook_id: self.id.clone(),
            name: name.clone(),
            logbook_type: self.logbook_type,
            schema_version: migrate::latest_version(LOGBOOK_MIGRATIONS),
            created_at_utc: now_utc(),
            app_version: APP_VERSION.into(),
        });
        d.name = name;
        write_descriptor(&self.dir, &d)?;
        self.info()
    }

    /// Archive state is stored in the database; files are never moved or deleted.
    pub fn set_archived(&mut self, archived: bool) -> AppResult<()> {
        self.check_storage()?;
        if !archived && self.view_copy.is_some() {
            self.upgrade_in_place()?;
        }
        let tx = self.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if archived {
            tx.execute("UPDATE logbook SET archived_at_utc=?1, updated_at_utc=?1 WHERE archived_at_utc IS NULL", params![now_utc()])?;
        } else {
            tx.execute("UPDATE logbook SET archived_at_utc=NULL, updated_at_utc=?1", params![now_utc()])?;
        }
        tx.commit()?;
        self.read_only = archived;
        Ok(())
    }

    pub fn stats(&self, day_start_utc: Option<&str>, day_end_utc: Option<&str>) -> AppResult<LogbookStats> {
        compute_stats(&self.conn, day_start_utc, day_end_utc)
    }

    /// Remove attachment files no committed entry references (left by failed saves) and
    /// staging files that no recoverable draft still references.
    pub fn cleanup_orphans(&self, keep_staging: &HashSet<String>) -> AppResult<usize> {
        if self.read_only {
            return Ok(0);
        }
        let mut referenced = HashSet::new();
        {
            let mut stmt = self.conn.prepare("SELECT rel_path FROM attachments")?;
            for r in stmt.query_map([], |r| r.get::<_, String>(0))? {
                referenced.insert(r?);
            }
        }
        let mut removed = 0;
        if let Ok(rd) = fs::read_dir(self.dir.join(ATTACHMENTS_DIR)) {
            for e in rd.flatten() {
                let fname = e.file_name().to_string_lossy().to_string();
                let rel = format!("{ATTACHMENTS_DIR}/{fname}");
                let is_ours = fname.starts_with(".tmp-") || is_attachment_name(&fname);
                if is_ours && !referenced.contains(&rel) && e.path().is_file() && fs::remove_file(e.path()).is_ok() {
                    removed += 1;
                }
            }
        }
        if let Ok(rd) = fs::read_dir(self.dir.join(STAGING_DIR)) {
            for e in rd.flatten() {
                let fname = e.file_name().to_string_lossy().to_string();
                let id = fname.trim_end_matches(".json");
                if !keep_staging.contains(id) && fs::remove_file(e.path()).is_ok() {
                    removed += 1;
                }
            }
        }
        Ok(removed)
    }

    /// Checkpoint the WAL into the main file and release the lock cleanly.
    /// Unarchiving asks to edit an archived older-version logbook: upgrade the real file now (keeping a
    /// verified pre-upgrade copy) and switch from the temporary view copy to it.
    fn upgrade_in_place(&mut self) -> AppResult<()> {
        let mut live = Connection::open(self.db_path())?;
        configure_connection(&live, false)?;
        let from = migrate::current_version(&live)?;
        let copy = pre_migration_copy(&live, &self.dir, from)?;
        migrate::migrate(&mut live, LOGBOOK_MIGRATIONS)?;
        let (name, ty, created): (String, String, String) =
            live.query_row("SELECT name, type, created_at_utc FROM logbook", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        write_descriptor(
            &self.dir,
            &Descriptor {
                format: "EMCOMM-LOG-LIVE".into(),
                descriptor_version: 1,
                logbook_id: self.id.clone(),
                name,
                logbook_type: LogbookType::parse(&ty).unwrap_or(self.logbook_type),
                schema_version: migrate::latest_version(LOGBOOK_MIGRATIONS),
                created_at_utc: created,
                app_version: APP_VERSION.into(),
            },
        )?;
        self.conn = live;
        if let Some(tmp) = self.view_copy.take() {
            let _ = fs::remove_file(tmp);
        }
        self.notices.push(format!("The logbook was upgraded in place. A copy of the previous version was kept at {}.", copy.display()));
        Ok(())
    }

    pub fn close(mut self) -> AppResult<()> {
        let _ = self.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
        if let Some(tmp) = self.view_copy.take() {
            let _ = fs::remove_file(tmp);
        }
        if let Some(l) = self.lock.take() {
            l.release_clean();
        }
        Ok(())
    }

    // ---------- attachments ----------

    pub fn stage_attachment(&self, src: &Path) -> AppResult<StagedAttachment> {
        self.ensure_writable()?;
        crate::faults::check("attachment.stage")?;
        let meta = fs::metadata(src).map_err(|e| AppError::from(e).with_context("Cannot read the selected file"))?;
        if !meta.is_file() {
            return Err(AppError::validation("attachments", "Only regular files can be attached"));
        }
        if meta.len() > MAX_ATTACHMENT_BYTES {
            return Err(AppError::validation("attachments", "Attachments are limited to 512 MB each"));
        }
        let staging = self.dir.join(STAGING_DIR);
        fs::create_dir_all(&staging)?;
        let id = fsutil::new_uuid();
        let data = staging.join(&id);
        let (sha, size) = match fsutil::copy_synced(src, &data) {
            Ok(v) => v,
            Err(e) => {
                let _ = fs::remove_file(&data);
                return Err(e);
            }
        };
        let display_name = src.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "attachment".into());
        let sa = StagedAttachment { staging_id: id.clone(), display_name, size_bytes: size, sha256: sha };
        fsutil::atomic_write(&staging.join(format!("{id}.json")), serde_json::to_string(&sa)?.as_bytes())?;
        Ok(sa)
    }

    pub fn staged_exists(&self, staging_id: &str) -> bool {
        uuid::Uuid::parse_str(staging_id).is_ok() && self.dir.join(STAGING_DIR).join(staging_id).is_file()
    }

    pub fn discard_staged(&self, staging_id: &str) {
        if uuid::Uuid::parse_str(staging_id).is_ok() {
            let _ = fs::remove_file(self.dir.join(STAGING_DIR).join(staging_id));
            let _ = fs::remove_file(self.dir.join(STAGING_DIR).join(format!("{staging_id}.json")));
        }
    }

    /// Absolute path of a committed attachment, validated to stay inside the live directory.
    pub fn attachment_path(&self, attachment_id: &str) -> AppResult<PathBuf> {
        let rel: String = self
            .conn
            .query_row("SELECT rel_path FROM attachments WHERE id=?1", params![attachment_id], |r| r.get(0))
            .optional()?
            .ok_or_else(|| AppError::not_found("Attachment not found"))?;
        if !fsutil::is_plain_relative(&rel) {
            return Err(AppError::new(ErrorCode::Integrity, "Attachment path is invalid"));
        }
        let p = self.dir.join(&rel);
        if !p.is_file() {
            return Err(AppError::new(ErrorCode::StorageUnavailable, "Attachment file is missing from the logbook folder"));
        }
        Ok(p)
    }
}

impl Drop for Logbook {
    fn drop(&mut self) {
        if let Some(tmp) = self.view_copy.take() {
            let _ = fs::remove_file(tmp);
        }
        // Unclean drop (e.g. panic) leaves lock content so the next open runs an integrity check.
        if let Some(l) = self.lock.take() {
            let _ = l.file.unlock();
        }
    }
}

pub fn is_attachment_name(name: &str) -> bool {
    let (stem, ext) = match name.split_once('.') {
        Some((s, e)) => (s, Some(e)),
        None => (name, None),
    };
    stem.len() == 64
        && stem.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        && ext.map(|e| !e.is_empty() && e.len() <= 10 && e.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())).unwrap_or(true)
}

#[derive(Debug, Clone, Serialize)]
pub struct StorageHealth {
    pub ok: bool,
    pub path: String,
    pub journal_mode: String,
    pub synchronous: i64,
    pub foreign_keys: bool,
    pub error: Option<AppError>,
}

pub fn read_settings(conn: &Connection) -> AppResult<LogbookSettings> {
    conn.query_row(
        "SELECT time_display_override, preset_id, preset_name, preset_version, preset_snapshot_json, operational_notes FROM logbook_settings",
        [],
        |r| {
            Ok(LogbookSettings {
                time_display_override: r.get(0)?,
                preset_id: r.get(1)?,
                preset_name: r.get(2)?,
                preset_version: r.get(3)?,
                preset: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or_default(),
                operational_notes: r.get(5)?,
            })
        },
    )
    .map_err(AppError::from)
}

pub fn compute_stats(conn: &Connection, day_start_utc: Option<&str>, day_end_utc: Option<&str>) -> AppResult<LogbookStats> {
    let mut s = LogbookStats::default();
    (s.total_active, s.total_void, s.flagged, s.first_event_utc, s.last_event_utc, s.last_recorded_utc, s.last_sequence) = conn.query_row(
        "SELECT COALESCE(SUM(status='active'),0), COALESCE(SUM(status='void'),0), COALESCE(SUM(status='active' AND flagged=1),0),
                    MIN(CASE WHEN status='active' THEN event_at_utc END), MAX(CASE WHEN status='active' THEN event_at_utc END),
                    MAX(recorded_at_utc), COALESCE(MAX(sequence_no),0)
             FROM entries",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
    )?;
    (s.important, s.open_follow_ups, s.overdue_follow_ups) = conn.query_row(
        "SELECT COALESCE(SUM(important=1),0), COALESCE(SUM(follow_up_state='open'),0),
                COALESCE(SUM(follow_up_state='open' AND follow_up_due_utc IS NOT NULL AND follow_up_due_utc < ?1),0)
         FROM entries WHERE status='active'",
        params![crate::fsutil::now_utc()],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if let (Some(a), Some(b)) = (day_start_utc, day_end_utc) {
        s.today = conn.query_row(
            "SELECT COUNT(*) FROM entries WHERE status='active' AND event_at_utc >= ?1 AND event_at_utc < ?2",
            params![a, b],
            |r| r.get(0),
        )?;
    }
    s.favorites =
        conn.query_row("SELECT COUNT(*) FROM entry_favorites f JOIN entries e ON e.id=f.entry_id WHERE e.status='active'", [], |r| {
            r.get(0)
        })?;
    s.stations = conn.query_row(
        "SELECT COUNT(DISTINCT upper(v)) FROM (
            SELECT from_text v FROM entries WHERE status='active' UNION ALL
            SELECT to_text FROM entries WHERE status='active' UNION ALL
            SELECT h.their_call FROM entry_ham h JOIN entries e ON e.id=h.entry_id WHERE e.status='active'
         ) WHERE trim(v) <> ''",
        [],
        |r| r.get(0),
    )?;
    let mut stmt =
        conn.prepare("SELECT handling, COUNT(*) FROM entries WHERE status='active' AND handling IS NOT NULL GROUP BY handling")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
        let (k, v) = row?;
        s.handling.insert(k, v);
    }
    Ok(s)
}
