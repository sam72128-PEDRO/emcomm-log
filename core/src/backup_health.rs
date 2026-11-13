//! Backup health and operator-initiated verification (2.3), over the existing backup engine.
//!
//! Nothing here creates, prunes, restores or repairs backups; the scheduler, manifest format and
//! retention in `backup.rs` / `snapshot.rs` are unchanged. Verification is strictly read-only: every
//! file is only read, and the backup database is opened read-only (`query_only`) — backups use the
//! rollback journal (`journal_mode=DELETE`), so a read-only open creates no side files.
//!
//! A backup is never reported as Verified because it exists, or because its creation-time check
//! passed: only a later full verification of that same backup folder sets Verified.

use crate::appdb::BackupRun;
use crate::error::{AppError, AppResult};
use crate::fsutil;
use crate::logbook::{ATTACHMENTS_DIR, DB_FILE, Logbook};
use crate::snapshot::{BACKUP_FORMAT, FORMAT_VERSION, MANIFEST_FILE, Manifest};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------- verification

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Passed,
    Failed,
    /// Could not run because an earlier step it depends on failed.
    NotRun,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerifyStep {
    pub id: String,
    pub label: String,
    pub status: StepStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerifyReport {
    pub path: String,
    pub logbook_id: Option<String>,
    pub verified_at_utc: String,
    pub backup_created_at_utc: Option<String>,
    pub backup_kind: Option<String>,
    /// Every step ran and passed.
    pub complete: bool,
    pub steps: Vec<VerifyStep>,
    /// One-line statement of exactly what was checked.
    pub scope: String,
    pub problems: Vec<String>,
    pub files_checked: usize,
    pub attachments_checked: usize,
    pub entry_count: Option<i64>,
}

pub const STEP_IDS: [(&str, &str); 7] = [
    ("access", "Backup folder and manifest readable"),
    ("manifest", "Manifest valid"),
    ("hashes", "SHA-256 of every file"),
    ("sqlite", "SQLite integrity check"),
    ("foreign_keys", "Database foreign-key check"),
    ("attachments", "Referenced attachments present and matching"),
    ("completeness", "Backup complete against its manifest"),
];

const MAX_PROBLEMS: usize = 50;

/// rel_path, sha256, size, display name, entry sequence number.
type AttachmentRow = (String, String, i64, String, i64);

struct Steps {
    steps: Vec<VerifyStep>,
    problems: Vec<String>,
}

impl Steps {
    fn new() -> Self {
        Steps {
            steps: STEP_IDS
                .iter()
                .map(|(id, label)| VerifyStep {
                    id: id.to_string(),
                    label: label.to_string(),
                    status: StepStatus::NotRun,
                    detail: String::new(),
                })
                .collect(),
            problems: Vec::new(),
        }
    }
    fn set(&mut self, id: &str, ok: bool, detail: impl Into<String>) {
        if let Some(s) = self.steps.iter_mut().find(|s| s.id == id) {
            s.status = if ok { StepStatus::Passed } else { StepStatus::Failed };
            s.detail = detail.into();
        }
    }
    fn problem(&mut self, p: impl Into<String>) {
        if self.problems.len() < MAX_PROBLEMS {
            self.problems.push(p.into());
        }
    }
}

/// Fully verify one backup folder. Never writes to it. The report states what passed, what failed,
/// and what could not be checked.
pub fn verify_backup(dir: &Path) -> VerifyReport {
    let mut st = Steps::new();
    let mut report = VerifyReport {
        path: dir.to_string_lossy().to_string(),
        logbook_id: None,
        verified_at_utc: fsutil::now_utc(),
        backup_created_at_utc: None,
        backup_kind: None,
        complete: false,
        steps: vec![],
        scope: String::new(),
        problems: vec![],
        files_checked: 0,
        attachments_checked: 0,
        entry_count: None,
    };
    let manifest = run_checks(dir, &mut st, &mut report);
    if let Some(m) = &manifest {
        report.logbook_id = Some(m.source.logbook_id.clone());
        report.backup_created_at_utc = Some(m.created_at_utc.clone());
        report.backup_kind = m.backup_kind.clone();
    }
    report.complete = st.steps.iter().all(|s| s.status == StepStatus::Passed);
    let passed: Vec<&str> = st.steps.iter().filter(|s| s.status == StepStatus::Passed).map(|s| s.label.as_str()).collect();
    let not_run = st.steps.iter().filter(|s| s.status == StepStatus::NotRun).count();
    report.scope = if report.complete {
        format!(
            "Complete verification: manifest, SHA-256 of {} file(s), SQLite integrity, foreign keys, {} referenced attachment(s), completeness.",
            report.files_checked, report.attachments_checked
        )
    } else {
        format!(
            "Incomplete: {} of {} checks passed{}{}.",
            passed.len(),
            STEP_IDS.len(),
            if passed.is_empty() { String::new() } else { format!(" ({})", passed.join("; ")) },
            if not_run > 0 { format!("; {not_run} could not be run") } else { String::new() }
        )
    };
    report.steps = st.steps;
    report.problems = st.problems;
    report
}

fn run_checks(dir: &Path, st: &mut Steps, report: &mut VerifyReport) -> Option<Manifest> {
    // 1. Access
    if !dir.is_dir() {
        st.set("access", false, "The backup folder does not exist or is not reachable");
        st.problem("Backup folder missing or inaccessible");
        return None;
    }
    let raw = match fs::read(dir.join(MANIFEST_FILE)) {
        Ok(r) => r,
        Err(e) => {
            st.set("access", false, format!("manifest.json could not be read: {e}"));
            st.problem("Manifest missing or unreadable");
            return None;
        }
    };
    if fs::read_dir(dir).is_err() {
        st.set("access", false, "The backup folder cannot be listed");
        return None;
    }
    st.set("access", true, "Folder and manifest readable");

    // 2. Manifest
    let m: Manifest = match serde_json::from_slice(&raw) {
        Ok(m) => m,
        Err(_) => {
            st.set("manifest", false, "manifest.json is malformed");
            st.problem("Manifest is malformed");
            return None;
        }
    };
    let latest_schema = crate::migrate::latest_version(crate::migrate::LOGBOOK_MIGRATIONS);
    let mut mproblems = Vec::new();
    if m.format != BACKUP_FORMAT {
        mproblems.push(format!("Not an EMCOMM LOG backup (format '{}')", m.format));
    }
    if m.format_version > FORMAT_VERSION || m.schema_version > latest_schema {
        mproblems.push("Created by a newer version of EMCOMM LOG; this version cannot verify it".to_string());
    }
    let mut seen = BTreeSet::new();
    for f in &m.files {
        if !fsutil::is_plain_relative(&f.path) {
            mproblems.push(format!("Unsafe path in manifest: {}", f.path));
        } else if f.path != DB_FILE && !f.path.starts_with(&format!("{ATTACHMENTS_DIR}/")) {
            mproblems.push(format!("Unexpected file in manifest: {}", f.path));
        }
        if !seen.insert(f.path.clone()) {
            mproblems.push(format!("Listed twice: {}", f.path));
        }
        if f.sha256.len() != 64 || !f.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            mproblems.push(format!("Invalid SHA-256 for {}", f.path));
        }
    }
    if !seen.contains(DB_FILE) {
        mproblems.push("Manifest lists no database".to_string());
    }
    if !mproblems.is_empty() {
        st.set("manifest", false, mproblems.join("; "));
        for p in mproblems {
            st.problem(p);
        }
        return Some(m);
    }
    st.set(
        "manifest",
        true,
        format!("Format {} v{}, schema {}, {} file(s) listed", m.format, m.format_version, m.schema_version, m.files.len()),
    );

    // 3. Hashes (every listed file) + unlisted files
    let mut bad_files = BTreeSet::new();
    for f in &m.files {
        match fsutil::sha256_file(&dir.join(&f.path)) {
            Ok((h, n)) if h == f.sha256 && n == f.bytes => {}
            Ok(_) => {
                bad_files.insert(f.path.clone());
                st.problem(format!("File does not match its SHA-256 / size: {}", f.path));
            }
            Err(_) => {
                bad_files.insert(f.path.clone());
                st.problem(format!("File missing or unreadable: {}", f.path));
            }
        }
    }
    report.files_checked = m.files.len();
    let mut unlisted = Vec::new();
    if let Ok(rd) = fs::read_dir(dir.join(ATTACHMENTS_DIR)) {
        for e in rd.flatten() {
            let rel = format!("{ATTACHMENTS_DIR}/{}", e.file_name().to_string_lossy());
            if !seen.contains(&rel) {
                unlisted.push(rel);
            }
        }
    }
    for u in &unlisted {
        st.problem(format!("File not listed in the manifest: {u}"));
    }
    let hashes_ok = bad_files.is_empty() && unlisted.is_empty();
    st.set(
        "hashes",
        hashes_ok,
        if hashes_ok {
            format!("{} file(s) match", m.files.len())
        } else {
            format!("{} file(s) missing or changed, {} unlisted", bad_files.len(), unlisted.len())
        },
    );

    // 4–7 need the database file.
    let db = dir.join(DB_FILE);
    if !db.is_file() {
        st.set("sqlite", false, "Database file missing");
        return Some(m);
    }
    let conn = match Logbook::open_read_only(&db) {
        Ok(c) => c,
        Err(e) => {
            st.set("sqlite", false, format!("Database cannot be opened: {}", e.message));
            st.problem("Database cannot be opened");
            return Some(m);
        }
    };
    match conn.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0)) {
        Ok(s) if s == "ok" => st.set("sqlite", true, "ok"),
        Ok(s) => {
            st.set("sqlite", false, s.clone());
            st.problem(format!("SQLite integrity check: {s}"));
            return Some(m);
        }
        Err(e) => {
            st.set("sqlite", false, e.to_string());
            st.problem("SQLite integrity check could not run (database damaged)");
            return Some(m);
        }
    }

    // 5. Foreign keys
    let fk: Result<Vec<String>, rusqlite::Error> = (|| {
        let mut stmt = conn.prepare("PRAGMA foreign_key_check")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect()
    })();
    match fk {
        Ok(v) if v.is_empty() => st.set("foreign_keys", true, "No violations"),
        Ok(v) => {
            let tables: BTreeSet<_> = v.iter().cloned().collect();
            st.set("foreign_keys", false, format!("{} violation(s) in {}", v.len(), tables.into_iter().collect::<Vec<_>>().join(", ")));
            st.problem(format!("{} foreign-key violation(s)", v.len()));
        }
        Err(e) => st.set("foreign_keys", false, e.to_string()),
    }

    // 6. Referenced attachments
    let listed: BTreeMap<&str, (&str, u64)> = m.files.iter().map(|f| (f.path.as_str(), (f.sha256.as_str(), f.bytes))).collect();
    let rows: Result<Vec<AttachmentRow>, rusqlite::Error> = (|| {
        let mut stmt = conn.prepare(
            "SELECT a.rel_path, a.sha256, a.size_bytes, a.display_name, COALESCE(e.sequence_no, 0)
             FROM attachments a LEFT JOIN entries e ON e.id = a.entry_id ORDER BY e.sequence_no",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?;
        rows.collect()
    })();
    let mut referenced = BTreeSet::new();
    match rows {
        Ok(rows) => {
            let mut missing = 0;
            for (rel, sha, size, name, seq) in &rows {
                referenced.insert(rel.clone());
                let ok = fsutil::is_plain_relative(rel)
                    && rel.starts_with(&format!("{ATTACHMENTS_DIR}/"))
                    && listed.get(rel.as_str()).is_some_and(|(h, n)| *h == sha && *n == *size as u64)
                    && !bad_files.contains(rel);
                if !ok {
                    missing += 1;
                    st.problem(format!("Attachment missing or damaged: {name} (entry #{seq})"));
                }
            }
            report.attachments_checked = rows.len();
            st.set(
                "attachments",
                missing == 0,
                if missing == 0 {
                    format!("{} attachment(s) present and matching", rows.len())
                } else {
                    format!("{missing} of {} missing or damaged", rows.len())
                },
            );
        }
        Err(e) => st.set("attachments", false, format!("Attachment records could not be read: {e}")),
    }

    // 7. Completeness against the manifest
    let mut cp = Vec::new();
    match conn.query_row(
        "SELECT (SELECT COUNT(*) FROM logbook), (SELECT id FROM logbook LIMIT 1), (SELECT schema_version FROM logbook LIMIT 1),
                (SELECT COUNT(*) FROM entries), (SELECT COALESCE(MAX(sequence_no), 0) FROM entries)",
        [],
        |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<u32>>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(4)?))
        },
    ) {
        Ok((n, id, schema, entries, max_seq)) => {
            report.entry_count = Some(entries);
            if n != 1 {
                cp.push(format!("Database holds {n} logbooks (expected 1)"));
            }
            if id.as_deref() != Some(m.source.logbook_id.as_str()) {
                cp.push("Database belongs to a different logbook than the manifest".into());
            }
            if schema != Some(m.schema_version) {
                cp.push("Database schema differs from the manifest".into());
            }
            if entries != m.entry_count || max_seq != m.max_sequence {
                cp.push(format!("Entries: database {entries} (last #{max_seq}), manifest {} (last #{})", m.entry_count, m.max_sequence));
            }
        }
        Err(e) => cp.push(format!("Logbook record could not be read: {e}")),
    }
    let listed_atts: BTreeSet<String> = m.files.iter().filter(|f| f.path != DB_FILE).map(|f| f.path.clone()).collect();
    let extra: Vec<_> = listed_atts.difference(&referenced).collect();
    if !extra.is_empty() {
        cp.push(format!("{} file(s) in the manifest are not referenced by the database", extra.len()));
    }
    for p in &cp {
        st.problem(p.clone());
    }
    st.set(
        "completeness",
        cp.is_empty(),
        if cp.is_empty() {
            format!("{} entries, last #{}; files match the database", m.entry_count, m.max_sequence)
        } else {
            cp.join("; ")
        },
    );
    Some(m)
}

// ---------------------------------------------------------------- verification records

/// The outcome of one verification, kept in the per-user application folder (never in the backup or
/// the logbook, so verifying changes neither).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerificationRecord {
    pub logbook_id: String,
    pub path: String,
    pub verified_at_utc: String,
    pub complete: bool,
    pub scope: String,
    pub problems: Vec<String>,
    pub backup_created_at_utc: Option<String>,
}

impl VerificationRecord {
    pub fn from_report(logbook_id: &str, r: &VerifyReport) -> Self {
        VerificationRecord {
            logbook_id: logbook_id.to_string(),
            path: r.path.clone(),
            verified_at_utc: r.verified_at_utc.clone(),
            complete: r.complete,
            scope: r.scope.clone(),
            problems: r.problems.iter().take(5).cloned().collect(),
            backup_created_at_utc: r.backup_created_at_utc.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct StoreFile {
    version: u32,
    records: Vec<VerificationRecord>,
}

pub const VERIFICATIONS_FILE: &str = "backup-verifications.json";
const KEEP_PER_LOGBOOK: usize = 10;

pub struct VerificationStore {
    path: PathBuf,
}

impl VerificationStore {
    pub fn new(app_data_dir: &Path) -> Self {
        VerificationStore { path: app_data_dir.join(VERIFICATIONS_FILE) }
    }

    fn load(&self) -> StoreFile {
        fs::read(&self.path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// Newest first.
    pub fn for_logbook(&self, logbook_id: &str) -> Vec<VerificationRecord> {
        let mut v: Vec<_> = self.load().records.into_iter().filter(|r| r.logbook_id == logbook_id).collect();
        v.sort_by(|a, b| b.verified_at_utc.cmp(&a.verified_at_utc));
        v
    }

    pub fn record(&self, rec: VerificationRecord) -> AppResult<()> {
        let mut f = self.load();
        f.version = 1;
        f.records.push(rec);
        f.records.sort_by(|a, b| b.verified_at_utc.cmp(&a.verified_at_utc));
        let mut per: BTreeMap<String, usize> = BTreeMap::new();
        f.records.retain(|r| {
            let n = per.entry(r.logbook_id.clone()).or_default();
            *n += 1;
            *n <= KEEP_PER_LOGBOOK
        });
        fsutil::atomic_write(&self.path, serde_json::to_string_pretty(&f)?.as_bytes())
    }
}

// ---------------------------------------------------------------- latest backup lookup

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LatestBackup {
    pub path: String,
    pub created_at_utc: String,
    pub kind: String,
    pub entry_count: i64,
}

/// The newest completed backup of `logbook_id` at the destination, from the manifests on disk
/// (authoritative even when the run history was cleared). Only manifests are read.
pub fn latest_backup(dest_root: &Path, logbook_id: &str) -> Option<LatestBackup> {
    let book = crate::backup::find_existing_book_dir(dest_root, logbook_id)?;
    let mut best: Option<LatestBackup> = None;
    for e in fs::read_dir(book).ok()?.flatten() {
        if e.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(raw) = fs::read(e.path().join(MANIFEST_FILE)) else { continue };
        let Ok(m) = serde_json::from_slice::<Manifest>(&raw) else { continue };
        if m.format != BACKUP_FORMAT || m.source.logbook_id != logbook_id {
            continue;
        }
        if best.as_ref().is_none_or(|b| m.created_at_utc > b.created_at_utc) {
            best = Some(LatestBackup {
                path: e.path().to_string_lossy().to_string(),
                created_at_utc: m.created_at_utc,
                kind: m.backup_kind.unwrap_or_default(),
                entry_count: m.entry_count,
            });
        }
    }
    best
}

// ---------------------------------------------------------------- health assessment

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    Verified,
    NotConfigured,
    Unavailable,
    AttentionRequired,
    VerificationFailed,
    Unknown,
}

impl HealthState {
    pub fn label(&self) -> &'static str {
        match self {
            HealthState::Verified => "Verified",
            HealthState::NotConfigured => "Not Configured",
            HealthState::Unavailable => "Unavailable",
            HealthState::AttentionRequired => "Attention Required",
            HealthState::VerificationFailed => "Verification Failed",
            HealthState::Unknown => "Unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupHealth {
    pub state: HealthState,
    pub state_label: String,
    pub headline: String,
    /// Every condition found, most important first.
    pub reasons: Vec<String>,
    pub destination: Option<String>,
    pub destination_accessible: Option<bool>,
    pub destination_message: Option<String>,
    pub interval_minutes: u32,
    pub on_close: bool,
    pub last_attempt: Option<BackupRun>,
    pub last_success: Option<BackupRun>,
    pub latest_backup: Option<LatestBackup>,
    pub backup_age_minutes: Option<i64>,
    /// The last time a scheduled backup was skipped because nothing had changed (not a failure).
    pub last_unchanged_check_utc: Option<String>,
    /// Verification of the latest backup, if it has been verified.
    pub latest_verification: Option<VerificationRecord>,
    /// The most recent verification of any backup of this logbook.
    pub last_verification_any: Option<VerificationRecord>,
    /// Some(true) when the live logbook has changed since the latest backup; None when unknown.
    pub changes_since_backup: Option<bool>,
    pub protection: String,
    pub last_error: Option<String>,
}

pub struct HealthInputs<'a> {
    pub destination: Option<&'a str>,
    /// Result of `backup::check_destination` (None when not configured).
    pub destination_check: Option<Result<(), AppError>>,
    pub runs: &'a [BackupRun],
    pub latest_backup: Option<LatestBackup>,
    pub verifications: &'a [VerificationRecord],
    pub live_marker: Option<String>,
    pub last_unchanged_check_utc: Option<String>,
    pub interval_minutes: u32,
    pub on_close: bool,
    pub now: DateTime<Utc>,
}

/// Grace before a schedule counts as overdue: two intervals plus five minutes.
pub fn overdue_after_minutes(interval: u32) -> i64 {
    i64::from(interval) * 2 + 5
}

pub fn assess(i: HealthInputs) -> BackupHealth {
    let last_attempt = i.runs.first().cloned();
    let last_success = i.runs.iter().find(|r| r.ok).cloned();
    let last_error = i.runs.iter().find(|r| !r.ok).map(|r| format!("{} ({})", r.message, r.finished_at_utc));
    let mut h = BackupHealth {
        state: HealthState::Unknown,
        state_label: String::new(),
        headline: String::new(),
        reasons: vec![],
        destination: i.destination.map(String::from),
        destination_accessible: None,
        destination_message: None,
        interval_minutes: i.interval_minutes,
        on_close: i.on_close,
        last_attempt: last_attempt.clone(),
        last_success: last_success.clone(),
        latest_backup: i.latest_backup.clone(),
        backup_age_minutes: None,
        last_unchanged_check_utc: i.last_unchanged_check_utc.clone(),
        latest_verification: None,
        last_verification_any: i.verifications.first().cloned(),
        changes_since_backup: None,
        protection: String::new(),
        last_error,
    };
    let finish = |mut h: BackupHealth, state: HealthState, headline: String| {
        h.state = state;
        h.state_label = state.label().into();
        h.headline = headline;
        h
    };
    let Some(dest) = i.destination else {
        h.protection = "No independent copy is being made. Logging works normally.".into();
        return finish(h, HealthState::NotConfigured, "No backup destination is configured".into());
    };
    let dest_err = i.destination_check.and_then(|r| r.err());
    if !Path::new(dest).is_dir() {
        h.destination_accessible = Some(false);
        h.destination_message = Some(dest_err.map(|e| e.message).unwrap_or_else(|| "Destination not found".into()));
        h.protection = "New backups cannot be made until the destination is reachable again.".into();
        return finish(h, HealthState::Unavailable, "The backup destination is missing or not reachable (drive disconnected?)".into());
    }
    h.destination_accessible = Some(dest_err.is_none());
    if let Some(e) = &dest_err {
        h.destination_message = Some(e.message.clone());
        h.reasons.push(format!("Destination problem: {}", e.message));
    }

    // Latest backup on disk and its age.
    if let Some(lb) = &i.latest_backup
        && let Some(t) = crate::validate::parse_utc(&lb.created_at_utc)
    {
        h.backup_age_minutes = Some((i.now - t).num_minutes().max(0));
    }
    // Changes since: compare with the marker recorded for that same backup.
    if let (Some(lb), Some(ls), Some(live)) = (&i.latest_backup, &last_success, &i.live_marker)
        && ls.path.as_deref() == Some(lb.path.as_str())
        && let Some(m) = &ls.data_marker
    {
        h.changes_since_backup = Some(m != live);
    }

    let verification = i.latest_backup.as_ref().and_then(|lb| i.verifications.iter().find(|v| v.path == lb.path).cloned());
    h.latest_verification = verification.clone();

    if i.latest_backup.is_none() {
        if last_success.is_some() {
            h.reasons.push("The most recent backup folder is no longer at the destination".into());
        } else {
            h.reasons.push("No backup of this logbook has been made yet".into());
        }
    }
    if let (Some(a), true) = (&last_attempt, last_attempt.as_ref().is_some_and(|a| !a.ok)) {
        h.reasons.insert(0, format!("Last backup attempt failed: {}", a.message));
    }
    if let (Some(age), Some(true)) = (h.backup_age_minutes, h.changes_since_backup)
        && age > overdue_after_minutes(i.interval_minutes)
    {
        h.reasons
            .push(format!("Backup overdue: changes have not been backed up for {age} min (schedule every {} min)", i.interval_minutes));
    }
    h.protection = match (&i.latest_backup, h.changes_since_backup) {
        (None, _) => "No independent copy of this logbook exists yet.".into(),
        (Some(_), Some(false)) => "The latest backup contains every change made so far.".into(),
        (Some(lb), Some(true)) => format!("Changes made after {} are not in any backup yet.", lb.created_at_utc),
        (Some(_), None) => "Whether the latest backup holds the newest changes cannot be determined.".into(),
    };

    if let Some(v) = &verification
        && !v.complete
    {
        let mut reasons = vec![format!(
            "Verification on {} failed: {}",
            v.verified_at_utc,
            v.problems.first().cloned().unwrap_or_else(|| v.scope.clone())
        )];
        reasons.append(&mut h.reasons);
        h.reasons = reasons;
        return finish(h, HealthState::VerificationFailed, "The latest backup failed its integrity check".into());
    }
    if !h.reasons.is_empty() {
        let head = h.reasons[0].clone();
        return finish(h, HealthState::AttentionRequired, head);
    }
    match verification {
        Some(v) => finish(h, HealthState::Verified, format!("Latest backup verified on {}", v.verified_at_utc)),
        None => finish(
            h,
            HealthState::Unknown,
            "A backup exists. It was checked when it was created, but has not been fully verified since. Use Verify Latest Backup.".into(),
        ),
    }
}
