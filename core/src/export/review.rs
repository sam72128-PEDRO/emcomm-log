//! 2.2 Review Export (EXP-*): show exactly which entries an export or ICS form would contain, let the
//! operator uncheck any, and refuse to write if a reviewed entry changed before the file is written.
//! Reviewing never changes an entry (no flags, statuses or revisions are touched).

use crate::entries::load_entry;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::models::{Entry, EntryStatus};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

/// Above this many entries the review shows counts only and the whole scope is exported.
pub const MAX_REVIEW_ROWS: usize = 10_000;

/// An entry as it was when reviewed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewRef {
    pub id: String,
    pub revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewRow {
    pub id: String,
    pub sequence_no: i64,
    pub revision: i64,
    pub kind: String,
    pub event_at_utc: String,
    pub subject: String,
    /// The other station(s): From → To, or the QSO callsign.
    pub station: String,
    pub operator: String,
    pub status: EntryStatus,
    pub eligible: bool,
    /// Why it cannot be included (empty when eligible).
    pub reason: String,
    pub attachments: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ExportReview {
    pub rows: Vec<ReviewRow>,
    pub eligible: usize,
    pub excluded: usize,
    /// True when there were more than MAX_REVIEW_ROWS: rows are not listed; the whole scope is exported.
    pub too_many: bool,
    pub total: usize,
    /// Format-specific notes shown above the list.
    pub notes: Vec<String>,
}

pub fn row(e: &Entry, eligible: Result<(), String>) -> ReviewRow {
    let station = match &e.ham {
        Some(h) if !h.their_call.is_empty() => h.their_call.clone(),
        _ => [e.from_text.as_str(), e.to_text.as_str()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" → "),
    };
    ReviewRow {
        id: e.id.clone(),
        sequence_no: e.sequence_no,
        revision: e.current_revision,
        kind: e.kind.as_str().into(),
        event_at_utc: e.event_at_utc.clone(),
        subject: e.subject.clone(),
        station,
        operator: e.operator_snapshot.name.clone(),
        status: e.status,
        eligible: eligible.is_ok(),
        reason: eligible.err().unwrap_or_default(),
        attachments: e.attachments.len(),
    }
}

pub fn build(rows: Vec<ReviewRow>, notes: Vec<String>) -> ExportReview {
    let total = rows.len();
    let eligible = rows.iter().filter(|r| r.eligible).count();
    if total > MAX_REVIEW_ROWS {
        return ExportReview { rows: vec![], eligible, excluded: total - eligible, too_many: true, total, notes };
    }
    ExportReview { excluded: total - eligible, eligible, rows, too_many: false, total, notes }
}

/// Refuse to write when a reviewed entry no longer matches what was reviewed (EXP-08).
pub fn check_unchanged(conn: &Connection, reviewed: &[ReviewRef]) -> AppResult<()> {
    let mut changed: Vec<String> = Vec::new();
    for r in reviewed {
        let now: Option<(i64, i64, String)> = conn
            .query_row("SELECT sequence_no, current_revision, status FROM entries WHERE id=?1", params![r.id], |x| {
                Ok((x.get(0)?, x.get(1)?, x.get(2)?))
            })
            .optional()?;
        match now {
            None => changed.push("an entry that no longer exists".into()),
            Some((seq, rev, status)) if rev != r.revision => {
                changed.push(format!("#{seq}{}", if status == "void" { " (voided)" } else { " (edited)" }))
            }
            _ => {}
        }
    }
    if changed.is_empty() {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::Conflict,
            format!("Changed since you reviewed the export: {}. Review it again; nothing was written.", changed.join(", ")),
        ))
    }
}

/// Load the reviewed entries in event order (the order every report uses).
pub fn load_reviewed(conn: &Connection, reviewed: &[ReviewRef]) -> AppResult<Vec<Entry>> {
    let mut v: Vec<Entry> = reviewed.iter().map(|r| load_entry(conn, &r.id)).collect::<AppResult<_>>()?;
    v.sort_by(|a, b| (&a.event_at_utc, a.sequence_no).cmp(&(&b.event_at_utc, b.sequence_no)));
    Ok(v)
}
