//! Station / assignment and operator shifts (schema v4, 2.1 spec §2).
//!
//! The station is the desk or position (tactical ID, licensed station callsign, agency, default
//! location); the operator is the person. Several people can operate one station in sequence: each
//! shift is an `operator_sessions` row. Entries record snapshots of both, so switching operators or
//! editing the station or a profile never rewrites history. Switching never touches operational
//! periods or ICS forms.

use crate::error::{AppError, AppResult};
use crate::fsutil::{self, now_utc};
use crate::logbook::Logbook;
use crate::models::{Location, OperatorSession, OperatorSnapshot, Station};
use crate::validate::{
    MAX_MEDIUM, MAX_SHORT, normalize_callsign, normalize_pota_ref, normalize_sota_ref, normalize_utc, parse_utc, single_line,
    validate_location,
};
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};

/// The logbook's station (an empty default when none has been set yet).
pub fn read_station(conn: &Connection) -> AppResult<Station> {
    let row = conn
        .query_row(
            "SELECT label, tactical_id, station_callsign, agency, default_location_json, pota_ref, sota_ref, updated_at_utc FROM station LIMIT 1",
            [],
            |r| {
                Ok(Station {
                    label: r.get(0)?,
                    tactical_id: r.get(1)?,
                    station_callsign: r.get(2)?,
                    agency: r.get(3)?,
                    default_location: r.get::<_, Option<String>>(4)?.and_then(|j| serde_json::from_str(&j).ok()),
                    pota_ref: r.get(5)?,
                    sota_ref: r.get(6)?,
                    updated_at_utc: r.get(7)?,
                })
            },
        )
        .optional()?;
    Ok(row.unwrap_or_default())
}

fn row_to_session(r: &Row) -> rusqlite::Result<OperatorSession> {
    let snap: String = r.get(2)?;
    Ok(OperatorSession {
        id: r.get(0)?,
        profile_id: r.get(1)?,
        operator: serde_json::from_str(&snap).unwrap_or_default(),
        started_at_utc: r.get(3)?,
        ended_at_utc: r.get(4)?,
        note: r.get(5)?,
    })
}

const SESSION_SELECT: &str = "SELECT id, profile_id, operator_snapshot_json, started_at_utc, ended_at_utc, note FROM operator_sessions";

pub fn list_sessions(conn: &Connection) -> AppResult<Vec<OperatorSession>> {
    let mut stmt = conn.prepare(&format!("{SESSION_SELECT} ORDER BY started_at_utc, created_at_utc"))?;
    let rows = stmt.query_map([], row_to_session)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

pub fn active_session(conn: &Connection) -> AppResult<Option<OperatorSession>> {
    Ok(conn
        .query_row(
            &format!("{SESSION_SELECT} WHERE ended_at_utc IS NULL ORDER BY started_at_utc DESC, created_at_utc DESC LIMIT 1"),
            [],
            row_to_session,
        )
        .optional()?)
}

fn insert_session(conn: &Connection, op: &OperatorSnapshot, start: &str, note: &str) -> AppResult<String> {
    let id = fsutil::new_uuid();
    conn.execute(
        "INSERT INTO operator_sessions(id, profile_id, operator_snapshot_json, started_at_utc, ended_at_utc, note, created_at_utc)
         VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6)",
        params![id, op.profile_id, serde_json::to_string(op)?, start, note, now_utc()],
    )?;
    Ok(id)
}

impl Logbook {
    pub fn station(&self) -> AppResult<Station> {
        read_station(&self.conn)
    }

    /// Change the station prospectively. Entries already saved keep the snapshot they were saved with.
    pub fn set_station(&mut self, s: &Station) -> AppResult<Station> {
        self.ensure_writable()?;
        let v = Station {
            label: single_line("station.label", &s.label, MAX_SHORT)?,
            tactical_id: single_line("station.tactical_id", &s.tactical_id, MAX_SHORT)?,
            station_callsign: normalize_callsign("station.station_callsign", &s.station_callsign)?,
            agency: single_line("station.agency", &s.agency, MAX_MEDIUM)?,
            default_location: s.default_location.as_ref().map(|l| validate_location("station.default_location", l)).transpose()?,
            pota_ref: normalize_pota_ref("station.pota_ref", &s.pota_ref)?,
            sota_ref: normalize_sota_ref("station.sota_ref", &s.sota_ref)?,
            updated_at_utc: now_utc(),
        };
        let loc = v.default_location.as_ref().map(serde_json::to_string).transpose()?;
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO station(logbook_id, label, tactical_id, station_callsign, agency, default_location_json, pota_ref, sota_ref, updated_at_utc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(logbook_id) DO UPDATE SET label=?2, tactical_id=?3, station_callsign=?4, agency=?5,
                default_location_json=?6, pota_ref=?7, sota_ref=?8, updated_at_utc=?9",
            params![self.id, v.label, v.tactical_id, v.station_callsign, v.agency, loc, v.pota_ref, v.sota_ref, v.updated_at_utc],
        )?;
        tx.execute("UPDATE logbook SET updated_at_utc=?1", params![v.updated_at_utc])?;
        tx.commit()?;
        read_station(&self.conn)
    }

    pub fn operator_sessions(&self) -> AppResult<Vec<OperatorSession>> {
        list_sessions(&self.conn)
    }

    pub fn active_operator(&self) -> AppResult<Option<OperatorSession>> {
        active_session(&self.conn)
    }

    /// The operator new entries are recorded under: the open shift, or — when the logbook has none yet
    /// — a first shift started now for `fallback` (the app's selected profile).
    pub fn ensure_operator(&mut self, fallback: &OperatorSnapshot) -> AppResult<OperatorSnapshot> {
        if let Some(s) = active_session(&self.conn)? {
            return Ok(s.operator);
        }
        self.ensure_writable()?;
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if active_session(&tx)?.is_none() {
            insert_session(&tx, fallback, &now_utc(), "")?;
        }
        tx.commit()?;
        Ok(active_session(&self.conn)?.map(|s| s.operator).unwrap_or_else(|| fallback.clone()))
    }

    /// Hand over the station to another operator at `at_utc` (default now). The outgoing shift ends and
    /// the incoming one starts at the same instant. Nothing already saved changes; operational periods
    /// and ICS forms are not touched.
    pub fn switch_operator(&mut self, incoming: &OperatorSnapshot, at_utc: Option<&str>, note: &str) -> AppResult<OperatorSession> {
        self.ensure_writable()?;
        if incoming.name.trim().is_empty() {
            return Err(AppError::validation("operator", "Choose the incoming operator"));
        }
        let note = single_line("note", note, MAX_MEDIUM)?;
        let now = now_utc();
        let at = match at_utc {
            Some(t) if !t.trim().is_empty() => normalize_utc("at_utc", t)?,
            _ => now.clone(),
        };
        if parse_utc(&at) > parse_utc(&now).map(|n| n + chrono::Duration::minutes(1)) {
            return Err(AppError::validation("at_utc", "The handover time cannot be in the future"));
        }
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(cur) = active_session(&tx)? {
            if parse_utc(&at) < parse_utc(&cur.started_at_utc) {
                return Err(AppError::validation("at_utc", "The handover time is before the current operator's shift began"));
            }
            if cur.operator.profile_id.is_some() && cur.operator.profile_id == incoming.profile_id {
                return Err(AppError::validation("operator", "That operator is already on duty"));
            }
            tx.execute("UPDATE operator_sessions SET ended_at_utc=?2 WHERE id=?1", params![cur.id, at])?;
        }
        let id = insert_session(&tx, incoming, &at, &note)?;
        tx.execute("UPDATE logbook SET updated_at_utc=?1", params![now])?;
        tx.commit()?;
        Ok(list_sessions(&self.conn)?.into_iter().find(|s| s.id == id).expect("just inserted"))
    }
}

/// A default location applied to an entry becomes its own snapshot, marked as coming from the
/// logbook default (the original source is kept in `provider_id`).
pub fn snapshot_default(default: &Location) -> Location {
    let mut l = default.clone();
    if l.source != crate::models::LocationSource::LogbookDefault {
        l.provider_id = format!("{} {}", l.source.as_str(), l.provider_id).trim().to_string();
        l.source = crate::models::LocationSource::LogbookDefault;
    }
    l
}
