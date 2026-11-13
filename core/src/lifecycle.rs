//! Communications traffic lifecycle (2.3). Read-only views over the existing handling status and
//! revision history — no second status system and no new stored states.
//!
//! The handling status (`entries.handling`) says where a message stands in its transmission /
//! acknowledgment lifecycle. It changes only when an operator saves it; nothing here or elsewhere sets
//! it as a side effect (linking, follow-ups, shift changes, flags and reports never touch it). Every
//! change is already an immutable revision with time, editor and reason, so the history below is
//! read from those revisions rather than stored again.

use crate::error::{AppError, AppResult};
use crate::models::{FollowUpState, Handling, OperatorSnapshot};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

impl Handling {
    /// Counted as pending traffic on the dashboard: not yet transmitted, or transmitted without a
    /// recorded acknowledgment or hand-off. Same set the dashboard has always counted.
    pub fn is_outstanding(&self) -> bool {
        matches!(self, Handling::Pending | Handling::Sent | Handling::AwaitingAck)
    }

    /// Position in the lifecycle; a change to a lower rank undoes recorded progress.
    pub fn rank(&self) -> u8 {
        match self {
            Handling::Pending => 0,
            Handling::Sent => 1,
            Handling::AwaitingAck | Handling::Relayed => 2,
            Handling::Acknowledged | Handling::Delivered => 3,
            Handling::Closed => 4,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Handling::Pending => "Pending",
            Handling::Sent => "Sent",
            Handling::AwaitingAck => "Awaiting ACK",
            Handling::Acknowledged => "Acknowledged",
            Handling::Relayed => "Relayed",
            Handling::Delivered => "Delivered",
            Handling::Closed => "Closed",
        }
    }

    /// Plain-language stage.
    pub fn stage(&self) -> &'static str {
        match self {
            Handling::Pending => "Logged, not yet transmitted",
            Handling::Sent => "Transmitted (no acknowledgment recorded)",
            Handling::AwaitingAck => "Transmitted, awaiting acknowledgment",
            Handling::Acknowledged => "Acknowledged",
            Handling::Relayed => "Relayed to another station",
            Handling::Delivered => "Delivered",
            Handling::Closed => "Closed",
        }
    }

    /// What, if anything, the station still has to do for this message.
    pub fn outstanding_action(&self) -> &'static str {
        match self {
            Handling::Pending => "Transmit the message",
            Handling::Sent => "None recorded. If an acknowledgment is expected, mark it Awaiting ACK",
            Handling::AwaitingAck => "Record the acknowledgment when it is received",
            _ => "",
        }
    }
}

/// True when moving from `before` to `after` would undo recorded transmission or acknowledgment
/// progress (for example Acknowledged back to Sent, or clearing the status of a transmitted message).
/// Such a correction is allowed but needs a reason, so the earlier status is never silently replaced.
pub fn is_regression(before: Option<Handling>, after: Option<Handling>) -> bool {
    match (before, after) {
        (Some(b), _) if b.rank() == 0 => false,
        (Some(_), None) => true,
        (Some(b), Some(a)) => a.rank() < b.rank(),
        (None, _) => false,
    }
}

/// Core guard used by Save Changes.
pub fn check_transition(before: Option<Handling>, after: Option<Handling>, reason: &str) -> AppResult<()> {
    if is_regression(before, after) && reason.trim().is_empty() {
        let from = before.map(|h| h.label()).unwrap_or("not tracked");
        let to = after.map(|h| h.label()).unwrap_or("not tracked");
        return Err(AppError::validation(
            "reason",
            format!(
                "This moves the message status back from {from} to {to}. Give a reason for the correction; \
                 the earlier status stays in the revision history."
            ),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HandlingChange {
    pub revision: i64,
    pub action: String,
    pub at_utc: String,
    pub from: Option<Handling>,
    pub to: Option<Handling>,
    /// Who saved the change (the revision's editor snapshot).
    pub by: OperatorSnapshot,
    pub reason: String,
    /// True when this change undid recorded progress (a correction).
    pub correction: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Lifecycle {
    pub entry_id: String,
    pub current: Option<Handling>,
    pub stage: String,
    pub outstanding_action: String,
    /// Counted in Pending Traffic.
    pub outstanding: bool,
    pub latest_change: Option<HandlingChange>,
    /// The change that recorded the current acknowledgment / delivery, when the status is one.
    pub acknowledgment: Option<HandlingChange>,
    pub changes: Vec<HandlingChange>,
    /// Shown alongside, never combined with, the message status.
    pub follow_up_state: FollowUpState,
    pub related_count: i64,
    pub voided: bool,
}

fn handling_of(v: &serde_json::Value) -> Option<Handling> {
    v.get("handling").and_then(|h| h.as_str()).and_then(Handling::parse)
}

/// Every revision that set or changed the handling status, oldest first.
pub fn handling_history(conn: &Connection, entry_id: &str) -> AppResult<Vec<HandlingChange>> {
    let revs = crate::entries::list_revisions(conn, entry_id)?;
    let mut out = Vec::new();
    for r in revs {
        let to = handling_of(&r.after);
        let from = r.before.as_ref().and_then(handling_of);
        let changed = if r.before.is_none() { to.is_some() } else { from != to };
        if changed {
            out.push(HandlingChange {
                revision: r.revision,
                action: r.action,
                at_utc: r.at_utc,
                from,
                to,
                by: r.editor,
                reason: r.reason,
                correction: is_regression(from, to),
            });
        }
    }
    Ok(out)
}

pub fn entry_lifecycle(conn: &Connection, entry_id: &str) -> AppResult<Lifecycle> {
    let (handling, fu, status): (Option<String>, String, String) =
        conn.query_row("SELECT handling, follow_up_state, status FROM entries WHERE id=?1", params![entry_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
    let related_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM entry_links WHERE removed_at_utc IS NULL AND (source_entry_id=?1 OR target_entry_id=?1)",
        params![entry_id],
        |r| r.get(0),
    )?;
    let current = handling.as_deref().and_then(Handling::parse);
    let changes = handling_history(conn, entry_id)?;
    let acknowledgment = match current {
        Some(Handling::Acknowledged | Handling::Delivered) => changes.iter().rev().find(|c| c.to == current).cloned(),
        _ => None,
    };
    Ok(Lifecycle {
        entry_id: entry_id.to_string(),
        current,
        stage: current.map(|h| h.stage()).unwrap_or("Not tracked").to_string(),
        outstanding_action: current.map(|h| h.outstanding_action()).unwrap_or("").to_string(),
        outstanding: status == "active" && current.map(|h| h.is_outstanding()).unwrap_or(false),
        latest_change: changes.last().cloned(),
        acknowledgment,
        changes,
        follow_up_state: FollowUpState::parse(&fu).unwrap_or_default(),
        related_count,
        voided: status == "void",
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LatestHandlingChange {
    pub entry_id: String,
    pub revision: i64,
    pub at_utc: String,
    pub by_name: String,
    pub by_callsign: String,
    pub to: Option<Handling>,
}

/// The latest revision that set or changed the handling status, for each of `ids` (one query; used by
/// list screens). Entries never given a status are absent.
pub fn latest_handling_changes(conn: &Connection, ids: &[String]) -> AppResult<Vec<LatestHandlingChange>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    if ids.len() > 1000 {
        return Err(AppError::validation("ids", "At most 1000 entries at a time"));
    }
    let marks = vec!["?"; ids.len()].join(",");
    let sql = format!(
        "SELECT r.entry_id, r.revision, r.at_utc,
                COALESCE(json_extract(r.editor_snapshot_json, '$.name'), ''),
                COALESCE(json_extract(r.editor_snapshot_json, '$.callsign'), ''),
                json_extract(r.after_json, '$.handling')
         FROM entry_revisions r
         WHERE r.entry_id IN ({marks})
           AND ((r.before_json IS NULL AND json_extract(r.after_json, '$.handling') IS NOT NULL)
             OR (r.before_json IS NOT NULL
                 AND COALESCE(json_extract(r.before_json, '$.handling'), '') <> COALESCE(json_extract(r.after_json, '$.handling'), '')))
           AND r.revision = (
             SELECT MAX(r2.revision) FROM entry_revisions r2
             WHERE r2.entry_id = r.entry_id
               AND ((r2.before_json IS NULL AND json_extract(r2.after_json, '$.handling') IS NOT NULL)
                 OR (r2.before_json IS NOT NULL
                     AND COALESCE(json_extract(r2.before_json, '$.handling'), '') <> COALESCE(json_extract(r2.after_json, '$.handling'), ''))))"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(ids.iter()), |r| {
        Ok(LatestHandlingChange {
            entry_id: r.get(0)?,
            revision: r.get(1)?,
            at_utc: r.get(2)?,
            by_name: r.get(3)?,
            by_callsign: r.get(4)?,
            to: r.get::<_, Option<String>>(5)?.as_deref().and_then(Handling::parse),
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Handling::*;

    #[test]
    fn regressions() {
        assert!(!is_regression(Some(Pending), Some(Sent)));
        assert!(!is_regression(Some(Sent), Some(AwaitingAck)));
        assert!(!is_regression(Some(AwaitingAck), Some(Acknowledged)));
        assert!(!is_regression(None, Some(Pending)));
        assert!(!is_regression(Some(Pending), None));
        assert!(!is_regression(Some(Relayed), Some(AwaitingAck)));
        assert!(is_regression(Some(Acknowledged), Some(Sent)));
        assert!(is_regression(Some(Acknowledged), None));
        assert!(is_regression(Some(Sent), Some(Pending)));
        assert!(is_regression(Some(Closed), Some(Delivered)));
        assert!(check_transition(Some(Acknowledged), Some(Pending), "  ").is_err());
        assert!(check_transition(Some(Acknowledged), Some(Pending), "Logged ACK on wrong message").is_ok());
    }

    #[test]
    fn outstanding_set_matches_dashboard() {
        let out: Vec<_> = Handling::ALL.iter().filter(|h| h.is_outstanding()).collect();
        assert_eq!(out, vec![&Pending, &Sent, &AwaitingAck]);
    }
}
