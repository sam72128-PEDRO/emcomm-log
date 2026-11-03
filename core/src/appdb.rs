//! Per-user application database: global settings, local operator profiles, presets, the
//! recent-logbook index and backup run history. None of this is exported in `.emlog`.

use crate::error::{AppError, AppResult, ErrorCode};
use crate::fsutil::{self, now_utc};
use crate::migrate::{self, APP_MIGRATIONS};
use crate::models::*;
use crate::validate;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::path::{Path, PathBuf};

pub const APP_DB_FILE: &str = "app.sqlite";

pub struct AppDb {
    pub conn: Connection,
    pub dir: PathBuf,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackupRun {
    pub logbook_id: String,
    pub kind: String,
    pub started_at_utc: String,
    pub finished_at_utc: String,
    pub ok: bool,
    pub path: Option<String>,
    pub entry_count: Option<i64>,
    pub message: String,
    pub data_marker: Option<String>,
}

fn seed_template(name: &str, d: TemplateDefaults) -> TemplateSeed {
    TemplateSeed { name: name.into(), defaults: d }
}

/// Built-in starter presets. Their templates are copied into a logbook on creation and
/// become ordinary logbook templates the operator can edit or delete.
pub fn builtin_presets() -> Vec<(String, LogbookType, PresetConfig)> {
    use EntryKind::*;
    let general = PresetConfig {
        default_kind: Some(Traffic),
        default_direction: Some(Direction::Received),
        default_priority: Some(Priority::Routine),
        quick_tags: vec!["INFO".into(), "MSG".into(), "STATUS".into()],
        ..Default::default()
    };
    let ham = PresetConfig {
        default_kind: Some(Qso),
        default_direction: Some(Direction::Other),
        default_priority: Some(Priority::Routine),
        default_mode: "SSB".into(),
        quick_tags: vec!["NET".into(), "DX".into(), "CONTEST".into(), "POTA".into()],
        templates: vec![seed_template(
            "Net check-in",
            TemplateDefaults {
                kind: Some(Traffic),
                direction: Some(Direction::Sent),
                subject: "Net check-in".into(),
                tags: vec!["NET".into()],
                ..Default::default()
            },
        )],
        ..Default::default()
    };
    let msg = |subject: &str, body: &str, tags: &[&str], pri: Priority| TemplateDefaults {
        kind: Some(Message),
        direction: Some(Direction::Received),
        priority: Some(pri),
        handling: Some(Handling::Pending),
        subject: subject.into(),
        body: body.into(),
        tags: tags.iter().map(|s| s.to_string()).collect(),
        emergency: Some(EmergencyFields { record_type: EmergencyRecordType::Message, ..Default::default() }),
        ..Default::default()
    };
    let emergency = PresetConfig {
        default_kind: Some(Message),
        default_direction: Some(Direction::Received),
        default_priority: Some(Priority::Routine),
        quick_tags: vec!["INFO".into(), "MSG".into(), "TX".into(), "RX".into(), "RELAY".into(), "STATUS".into()],
        templates: vec![
            seed_template(
                "Shelter Update",
                msg("Shelter status update", "Occupants: \nPets: \nCapacity: \nNeeds: \nIssues: ", &["STATUS"], Priority::Routine),
            ),
            seed_template(
                "Welfare Check",
                msg("Welfare check request", "Name: \nAddress: \nLast contact: \nRequested by: ", &["MSG"], Priority::Priority),
            ),
            seed_template(
                "Resource Request",
                msg("Resource request", "Item / quantity: \nDeliver to: \nNeeded by: \nPoint of contact: ", &["MSG"], Priority::Priority),
            ),
            seed_template(
                "Situation Report",
                TemplateDefaults {
                    kind: Some(Status),
                    direction: Some(Direction::Internal),
                    subject: "Situation report".into(),
                    body: "Conditions: \nComms status: \nActions taken: \nConcerns: ".into(),
                    tags: vec!["STATUS".into()],
                    emergency: Some(EmergencyFields::default()),
                    ..Default::default()
                },
            ),
        ],
        ..Default::default()
    };
    vec![
        ("General - Standard".into(), LogbookType::General, general),
        ("HAM - Standard".into(), LogbookType::Ham, ham),
        ("Emergency - Standard".into(), LogbookType::Emergency, emergency),
    ]
}

impl AppDb {
    pub fn open(dir: &Path) -> AppResult<AppDb> {
        std::fs::create_dir_all(dir)?;
        let mut conn = Connection::open(dir.join(APP_DB_FILE))?;
        crate::logbook::configure_connection(&conn, false)?;
        migrate::migrate(&mut conn, APP_MIGRATIONS)?;
        let db = AppDb { conn, dir: dir.to_path_buf() };
        db.seed_builtin_presets()?;
        Ok(db)
    }

    fn seed_builtin_presets(&self) -> AppResult<()> {
        let n: i64 = self.conn.query_row("SELECT COUNT(*) FROM presets WHERE builtin=1", [], |r| r.get(0))?;
        if n > 0 {
            return Ok(());
        }
        let now = now_utc();
        for (name, ty, cfg) in builtin_presets() {
            let cfg = validate::validate_preset_config(&cfg, ty)?;
            self.conn.execute(
                "INSERT INTO presets(id, name, logbook_type, profile_id, version, config_json, builtin, created_at_utc, updated_at_utc)
                 VALUES (?1, ?2, ?3, NULL, 1, ?4, 1, ?5, ?5)",
                params![fsutil::new_uuid(), name, ty.as_str(), serde_json::to_string(&cfg)?, now],
            )?;
        }
        Ok(())
    }

    // ---------- settings ----------

    pub fn settings(&self) -> AppResult<GlobalSettings> {
        let raw: Option<String> = self.conn.query_row("SELECT settings_json FROM app_settings WHERE id=1", [], |r| r.get(0)).optional()?;
        match raw {
            None => Ok(GlobalSettings::default()),
            Some(s) => {
                let mut g: GlobalSettings = serde_json::from_str(&s)
                    .map_err(|e| AppError::new(ErrorCode::Integrity, format!("Settings file is unreadable: {e}")))?;
                if g.schema_version > SETTINGS_SCHEMA_VERSION {
                    return Err(AppError::new(ErrorCode::UnsupportedVersion, "Settings were written by a newer version of EMCOMM LOG"));
                }
                g.schema_version = SETTINGS_SCHEMA_VERSION;
                Ok(g)
            }
        }
    }

    pub fn validate_settings(&self, s: &GlobalSettings) -> AppResult<GlobalSettings> {
        let mut s = s.clone();
        if s.time_display != "local" && s.time_display != "utc" {
            return Err(AppError::validation("time_display", "Choose local or UTC"));
        }
        if s.clock != "24h" && s.clock != "12h" {
            return Err(AppError::validation("clock", "Choose 12 or 24 hour"));
        }
        if !["dark", "light", "system"].contains(&s.theme.as_str()) {
            return Err(AppError::validation("theme", "Unknown theme"));
        }
        let b = &s.backup;
        if !(5..=1440).contains(&b.interval_minutes) {
            return Err(AppError::validation("backup.interval_minutes", "Backup interval must be 5-1440 minutes"));
        }
        if b.keep_recent < 1 || b.keep_recent > 500 || b.keep_daily > 365 || b.keep_weekly > 520 {
            return Err(AppError::validation("backup.keep_recent", "Retention values are out of range (keep at least 1 recent)"));
        }
        if let Some(p) = s.default_logbook_parent.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
            let path = Path::new(p);
            fsutil::check_writable(path).map_err(|e| AppError { field: Some("default_logbook_parent".into()), ..e })?;
            crate::logbook::check_live_location(path).map_err(|e| AppError { field: Some("default_logbook_parent".into()), ..e })?;
            s.default_logbook_parent = Some(p.to_string());
        } else {
            s.default_logbook_parent = None;
        }
        if let Some(p) = s.backup.destination.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
            fsutil::check_writable(Path::new(p)).map_err(|e| AppError { field: Some("backup.destination".into()), ..e })?;
            for row in self.list_index()? {
                if fsutil::is_within(Path::new(p), Path::new(&row.path)) {
                    return Err(AppError::validation(
                        "backup.destination",
                        format!("The backup destination is inside the live logbook '{}'. Choose an independent folder.", row.name),
                    ));
                }
            }
            if Path::new(p).join(crate::logbook::DB_FILE).exists() {
                return Err(AppError::validation("backup.destination", "The backup destination cannot be a live logbook folder"));
            }
            s.backup.destination = Some(p.to_string());
        } else {
            s.backup.destination = None;
        }
        if let Some(pid) = &s.active_profile_id {
            let p = self.get_profile(pid)?;
            if !p.active {
                return Err(AppError::validation("active_profile_id", "The selected profile is deactivated"));
            }
        }
        s.schema_version = SETTINGS_SCHEMA_VERSION;
        Ok(s)
    }

    pub fn save_settings(&mut self, s: &GlobalSettings) -> AppResult<GlobalSettings> {
        let s = self.validate_settings(s)?;
        crate::faults::check("settings.write")?;
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO app_settings(id, settings_json, updated_at_utc) VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET settings_json=excluded.settings_json, updated_at_utc=excluded.updated_at_utc",
            params![serde_json::to_string(&s)?, now_utc()],
        )?;
        tx.commit()?;
        self.settings()
    }

    // ---------- profiles ----------

    fn validate_profile(p: &ProfileInput) -> AppResult<ProfileInput> {
        let name = p.name.trim();
        if name.is_empty() || name.chars().count() > 80 {
            return Err(AppError::validation("name", "Operator name is required (max 80 characters)"));
        }
        let short = |f: &str, v: &str, max: usize| -> AppResult<String> {
            let t = v.trim();
            if t.chars().count() > max {
                return Err(AppError::validation(f, format!("At most {max} characters")));
            }
            Ok(t.to_string())
        };
        Ok(ProfileInput {
            name: name.to_string(),
            callsign: validate::normalize_callsign("callsign", &p.callsign)?,
            license_class: short("license_class", &p.license_class, 40)?,
            organization: short("organization", &p.organization, 120)?,
            position: short("position", &p.position, 80)?,
            grid: validate::normalize_grid("grid", &p.grid)?,
        })
    }

    pub fn list_profiles(&self) -> AppResult<Vec<Profile>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, callsign, license_class, organization, position, grid, active, created_at_utc, updated_at_utc
             FROM profiles ORDER BY active DESC, name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Profile {
                id: r.get(0)?,
                name: r.get(1)?,
                callsign: r.get(2)?,
                license_class: r.get(3)?,
                organization: r.get(4)?,
                position: r.get(5)?,
                grid: r.get(6)?,
                active: r.get(7)?,
                created_at_utc: r.get(8)?,
                updated_at_utc: r.get(9)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    pub fn get_profile(&self, id: &str) -> AppResult<Profile> {
        self.list_profiles()?.into_iter().find(|p| p.id == id).ok_or_else(|| AppError::not_found("Operator profile not found"))
    }

    pub fn create_profile(&mut self, p: &ProfileInput) -> AppResult<Profile> {
        let p = Self::validate_profile(p)?;
        let id = fsutil::new_uuid();
        let now = now_utc();
        self.conn.execute(
            "INSERT INTO profiles(id, name, callsign, license_class, organization, position, grid, active, created_at_utc, updated_at_utc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?8)",
            params![id, p.name, p.callsign, p.license_class, p.organization, p.position, p.grid, now],
        )?;
        self.get_profile(&id)
    }

    /// Editing a profile never rewrites logbooks: logbooks and entries hold their own snapshots.
    pub fn update_profile(&mut self, id: &str, p: &ProfileInput) -> AppResult<Profile> {
        let p = Self::validate_profile(p)?;
        let n = self.conn.execute(
            "UPDATE profiles SET name=?2, callsign=?3, license_class=?4, organization=?5, position=?6, grid=?7, updated_at_utc=?8 WHERE id=?1",
            params![id, p.name, p.callsign, p.license_class, p.organization, p.position, p.grid, now_utc()],
        )?;
        if n == 0 {
            return Err(AppError::not_found("Operator profile not found"));
        }
        self.get_profile(id)
    }

    pub fn set_profile_active(&mut self, id: &str, active: bool) -> AppResult<Profile> {
        if !active {
            let s = self.settings()?;
            if s.active_profile_id.as_deref() == Some(id) {
                return Err(AppError::validation("active", "Select a different active profile before deactivating this one"));
            }
        }
        let n = self.conn.execute("UPDATE profiles SET active=?2, updated_at_utc=?3 WHERE id=?1", params![id, active, now_utc()])?;
        if n == 0 {
            return Err(AppError::not_found("Operator profile not found"));
        }
        self.get_profile(id)
    }

    // ---------- presets ----------

    pub fn list_presets(&self) -> AppResult<Vec<Preset>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.id, p.name, p.logbook_type, p.profile_id, p.version, p.config_json, p.builtin, p.created_at_utc, p.updated_at_utc,
                    EXISTS(SELECT 1 FROM logbook_index i WHERE i.preset_id = p.id)
             FROM presets p ORDER BY p.logbook_type, p.builtin DESC, p.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Preset {
                id: r.get(0)?,
                name: r.get(1)?,
                logbook_type: LogbookType::parse(&r.get::<_, String>(2)?).unwrap_or(LogbookType::General),
                profile_id: r.get(3)?,
                version: r.get(4)?,
                config: serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_default(),
                builtin: r.get(6)?,
                created_at_utc: r.get(7)?,
                updated_at_utc: r.get(8)?,
                in_use: r.get(9)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    pub fn get_preset(&self, id: &str) -> AppResult<Preset> {
        self.list_presets()?.into_iter().find(|p| p.id == id).ok_or_else(|| AppError::not_found("Preset not found"))
    }

    fn check_preset_input(&self, p: &PresetInput) -> AppResult<(String, LogbookType, PresetConfig)> {
        let name = p.name.trim();
        if name.is_empty() || name.chars().count() > 80 {
            return Err(AppError::validation("name", "Preset name is required (max 80 characters)"));
        }
        let ty = p.logbook_type.ok_or_else(|| AppError::validation("logbook_type", "Choose a logbook type"))?;
        if let Some(pid) = &p.profile_id {
            self.get_profile(pid)?;
        }
        Ok((name.to_string(), ty, validate::validate_preset_config(&p.config, ty)?))
    }

    pub fn create_preset(&mut self, p: &PresetInput) -> AppResult<Preset> {
        let (name, ty, cfg) = self.check_preset_input(p)?;
        let id = fsutil::new_uuid();
        let now = now_utc();
        self.conn.execute(
            "INSERT INTO presets(id, name, logbook_type, profile_id, version, config_json, builtin, created_at_utc, updated_at_utc)
             VALUES (?1, ?2, ?3, ?4, 1, ?5, 0, ?6, ?6)",
            params![id, name, ty.as_str(), p.profile_id, serde_json::to_string(&cfg)?, now],
        )?;
        self.get_preset(&id)
    }

    /// Editing bumps the version. Existing logbooks keep the snapshot they were created with.
    pub fn update_preset(&mut self, id: &str, p: &PresetInput) -> AppResult<Preset> {
        let (name, ty, cfg) = self.check_preset_input(p)?;
        let current = self.get_preset(id)?;
        if current.logbook_type != ty {
            return Err(AppError::validation("logbook_type", "A preset's logbook type cannot change; duplicate it instead"));
        }
        self.conn.execute(
            "UPDATE presets SET name=?2, profile_id=?3, version=version+1, config_json=?4, updated_at_utc=?5 WHERE id=?1",
            params![id, name, p.profile_id, serde_json::to_string(&cfg)?, now_utc()],
        )?;
        self.get_preset(id)
    }

    pub fn duplicate_preset(&mut self, id: &str) -> AppResult<Preset> {
        let src = self.get_preset(id)?;
        let mut name = format!("{} (copy)", src.name);
        name = name.chars().take(80).collect();
        self.create_preset(&PresetInput { name, logbook_type: Some(src.logbook_type), profile_id: src.profile_id, config: src.config })
    }

    pub fn delete_preset(&mut self, id: &str) -> AppResult<()> {
        let p = self.get_preset(id)?;
        if p.in_use {
            return Err(AppError::new(
                ErrorCode::Conflict,
                "This preset was used to create a logbook on this computer and cannot be deleted",
            ));
        }
        self.conn.execute("DELETE FROM presets WHERE id=?1", params![id])?;
        Ok(())
    }

    // ---------- logbook index ----------

    pub fn list_index(&self) -> AppResult<Vec<LogbookIndexRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, name, type, incident_name, location, preset_id, source_uuid, entry_count, first_event_utc, last_event_utc,
                    last_modified_utc, last_opened_utc, archived, registered_at_utc
             FROM logbook_index ORDER BY archived, COALESCE(last_opened_utc, registered_at_utc) DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            let path: String = r.get(1)?;
            let available = Path::new(&path).join(crate::logbook::DB_FILE).is_file();
            Ok(LogbookIndexRow {
                id: r.get(0)?,
                path,
                name: r.get(2)?,
                logbook_type: LogbookType::parse(&r.get::<_, String>(3)?).unwrap_or(LogbookType::General),
                incident_name: r.get(4)?,
                location: r.get(5)?,
                preset_id: r.get(6)?,
                source_uuid: r.get(7)?,
                entry_count: r.get(8)?,
                first_event_utc: r.get(9)?,
                last_event_utc: r.get(10)?,
                last_modified_utc: r.get(11)?,
                last_opened_utc: r.get(12)?,
                archived: r.get(13)?,
                registered_at_utc: r.get(14)?,
                available,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    pub fn index_contains(&self, id: &str) -> AppResult<bool> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM logbook_index WHERE id=?1", params![id], |r| r.get::<_, i64>(0))? > 0)
    }

    pub fn index_by_path(&self, path: &Path) -> AppResult<Option<LogbookIndexRow>> {
        let canon = fsutil::canonical_or_self(path);
        Ok(self.list_index()?.into_iter().find(|r| fsutil::canonical_or_self(Path::new(&r.path)) == canon))
    }

    pub fn register_logbook(
        &mut self,
        info: &LogbookInfo,
        stats: &LogbookStats,
        preset_id: Option<&str>,
        source_uuid: Option<&str>,
    ) -> AppResult<()> {
        let now = now_utc();
        self.conn.execute(
            "INSERT INTO logbook_index(id, path, name, type, incident_name, location, preset_id, source_uuid, entry_count,
                first_event_utc, last_event_utc, last_modified_utc, last_opened_utc, archived, registered_at_utc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, NULL, ?13, ?14)
             ON CONFLICT(id) DO UPDATE SET path=excluded.path, name=excluded.name, incident_name=excluded.incident_name,
                location=excluded.location, entry_count=excluded.entry_count, first_event_utc=excluded.first_event_utc,
                last_event_utc=excluded.last_event_utc, last_modified_utc=excluded.last_modified_utc, archived=excluded.archived,
                preset_id=COALESCE(logbook_index.preset_id, excluded.preset_id),
                source_uuid=COALESCE(logbook_index.source_uuid, excluded.source_uuid)",
            params![
                info.id,
                info.path,
                info.name,
                info.logbook_type.as_str(),
                info.incident.incident_name,
                info.incident.location,
                preset_id,
                source_uuid,
                stats.total_active,
                stats.first_event_utc,
                stats.last_event_utc,
                info.updated_at_utc,
                info.archived_at_utc.is_some(),
                now
            ],
        )?;
        Ok(())
    }

    pub fn touch_opened(&mut self, id: &str) -> AppResult<()> {
        self.conn.execute("UPDATE logbook_index SET last_opened_utc=?2 WHERE id=?1", params![id, now_utc()])?;
        Ok(())
    }

    pub fn relink(&mut self, id: &str, new_path: &Path) -> AppResult<()> {
        self.conn.execute("UPDATE logbook_index SET path=?2 WHERE id=?1", params![id, new_path.to_string_lossy()])?;
        Ok(())
    }

    /// Forget a logbook in the local index only; never touches its files.
    pub fn forget_logbook(&mut self, id: &str) -> AppResult<()> {
        self.conn.execute("DELETE FROM logbook_index WHERE id=?1", params![id])?;
        Ok(())
    }

    // ---------- backups ----------

    pub fn record_backup(&mut self, run: &BackupRun) -> AppResult<()> {
        self.conn.execute(
            "INSERT INTO backup_runs(logbook_id, kind, started_at_utc, finished_at_utc, ok, path, entry_count, message, data_marker)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                run.logbook_id,
                run.kind,
                run.started_at_utc,
                run.finished_at_utc,
                run.ok,
                run.path,
                run.entry_count,
                run.message,
                run.data_marker
            ],
        )?;
        Ok(())
    }

    pub fn backup_runs(&self, logbook_id: &str, limit: u32) -> AppResult<Vec<BackupRun>> {
        let mut stmt = self.conn.prepare(
            "SELECT logbook_id, kind, started_at_utc, finished_at_utc, ok, path, entry_count, message, data_marker
             FROM backup_runs WHERE logbook_id=?1 ORDER BY rid DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![logbook_id, limit], |r| {
            Ok(BackupRun {
                logbook_id: r.get(0)?,
                kind: r.get(1)?,
                started_at_utc: r.get(2)?,
                finished_at_utc: r.get(3)?,
                ok: r.get(4)?,
                path: r.get(5)?,
                entry_count: r.get(6)?,
                message: r.get(7)?,
                data_marker: r.get(8)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    pub fn last_successful_backup(&self, logbook_id: &str) -> AppResult<Option<BackupRun>> {
        Ok(self.backup_runs(logbook_id, 200)?.into_iter().find(|r| r.ok))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_validation() {
        let d = tempfile::tempdir().unwrap();
        let mut db = AppDb::open(d.path()).unwrap();
        let mut s = db.settings().unwrap();
        assert!(!s.wizard_completed);
        s.wizard_completed = true;
        s.time_display = "utc".into();
        let saved = db.save_settings(&s).unwrap();
        assert_eq!(saved.time_display, "utc");
        drop(db);
        let db = AppDb::open(d.path()).unwrap();
        assert!(db.settings().unwrap().wizard_completed);
        let mut bad = s.clone();
        bad.backup.interval_minutes = 1;
        assert!(db.validate_settings(&bad).is_err());
    }

    #[test]
    fn builtin_presets_seeded_once() {
        let d = tempfile::tempdir().unwrap();
        let db = AppDb::open(d.path()).unwrap();
        assert_eq!(db.list_presets().unwrap().len(), 3);
        drop(db);
        let db = AppDb::open(d.path()).unwrap();
        assert_eq!(db.list_presets().unwrap().len(), 3);
    }

    #[test]
    fn profile_crud_and_deactivate_guard() {
        let d = tempfile::tempdir().unwrap();
        let mut db = AppDb::open(d.path()).unwrap();
        let p = db.create_profile(&ProfileInput { name: "Sam".into(), callsign: "kj4mrc".into(), ..Default::default() }).unwrap();
        assert_eq!(p.callsign, "KJ4MRC");
        let mut s = db.settings().unwrap();
        s.active_profile_id = Some(p.id.clone());
        db.save_settings(&s).unwrap();
        assert!(db.set_profile_active(&p.id, false).is_err());
        assert!(db.create_profile(&ProfileInput { name: "".into(), ..Default::default() }).is_err());
    }
}
