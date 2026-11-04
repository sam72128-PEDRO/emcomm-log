use crate::error::{AppError, AppResult, ErrorCode};
use crate::faults;
use crate::fsutil::{self, now_utc};
use crate::logbook::{ATTACHMENTS_DIR, Logbook, STAGING_DIR};
use crate::models::*;
use crate::validate::{validate_entry_input, validate_template_defaults};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params, params_from_iter};
use std::fs;
use std::path::{Path, PathBuf};

pub const MAX_PAGE: u32 = 500;

const ENTRY_SELECT: &str = "SELECT e.id, e.sequence_no, e.kind, e.event_at_utc, e.event_timezone, e.event_utc_offset_min,
    e.recorded_at_utc, e.updated_at_utc, e.status, e.voided_at_utc, e.void_reason, e.direction, e.priority, e.handling,
    e.flagged, e.from_text, e.to_text, e.subject, e.body, e.channel, e.location, e.operator_snapshot_json, e.tags_json,
    e.current_revision, e.op_id, e.station_snapshot_json, e.operator_location_json, e.reported_location_json,
    e.important, e.follow_up_state, e.follow_up_due_utc, e.follow_up_note, e.follow_up_resolved_utc,
    (SELECT COUNT(*) FROM entry_links l WHERE l.removed_at_utc IS NULL
        AND (l.source_entry_id = e.id OR l.target_entry_id = e.id)),
    (f.entry_id IS NOT NULL),
    h.entry_id, h.their_call, h.station_call, h.operator_call, h.qso_start_utc, h.qso_end_utc, h.frequency_hz, h.band,
    h.mode, h.submode, h.rst_sent, h.rst_rcvd, h.my_grid, h.their_grid, h.qso_notes,
    h.my_pota_ref, h.pota_ref, h.my_sota_ref, h.sota_ref,
    m.entry_id, m.record_type, m.incident_name, m.incident_number, m.operational_period, m.message_number, m.precedence,
    m.originator, m.originator_position, m.recipient, m.recipient_position, m.sender, m.receiver, m.sent_at_utc,
    m.received_at_utc, m.reply_requested, m.action_requested, m.response, m.resource_unit, m.assignment,
    m.activity_start_utc, m.activity_end_utc, m.location, m.disposition, m.preparer,
    m.approved_by_name, m.approved_by_position, m.replied_by_name, m.replied_by_position, m.replied_at_utc
  FROM entries e
  LEFT JOIN entry_favorites f ON f.entry_id = e.id
  LEFT JOIN entry_ham h ON h.entry_id = e.id
  LEFT JOIN entry_emergency m ON m.entry_id = e.id";

fn enum_col<T>(s: String, parse: fn(&str) -> Option<T>) -> rusqlite::Result<T> {
    parse(&s).ok_or_else(|| rusqlite::Error::InvalidColumnType(0, s, rusqlite::types::Type::Text))
}

fn row_to_entry(r: &Row) -> rusqlite::Result<Entry> {
    let mut c = 0usize;
    macro_rules! g {
        () => {{
            let v = r.get(c)?;
            c += 1;
            v
        }};
    }
    let id: String = g!();
    let sequence_no: i64 = g!();
    let kind = enum_col(g!(), EntryKind::parse)?;
    let event_at_utc: String = g!();
    let event_timezone: String = g!();
    let event_utc_offset_min: i32 = g!();
    let recorded_at_utc: String = g!();
    let updated_at_utc: String = g!();
    let status = enum_col(g!(), EntryStatus::parse)?;
    let voided_at_utc: Option<String> = g!();
    let void_reason: Option<String> = g!();
    let direction = enum_col(g!(), Direction::parse)?;
    let priority = enum_col(g!(), Priority::parse)?;
    let handling: Option<String> = g!();
    let flagged: bool = g!();
    let from_text: String = g!();
    let to_text: String = g!();
    let subject: String = g!();
    let body: String = g!();
    let channel: String = g!();
    let location: String = g!();
    let op_json: String = g!();
    let tags_json: String = g!();
    let current_revision: i64 = g!();
    let op_id: Option<String> = g!();
    let station_json: String = g!();
    let op_loc_json: Option<String> = g!();
    let rep_loc_json: Option<String> = g!();
    let important: bool = g!();
    let fu_state: String = g!();
    let follow_up = FollowUp { state: FollowUpState::parse(&fu_state).unwrap_or_default(), due_utc: g!(), note: g!(), resolved_utc: g!() };
    let related_count: i64 = g!();
    let favorite: bool = g!();
    let ham_id: Option<String> = g!();
    let ham = if ham_id.is_some() {
        Some(HamFields {
            their_call: g!(),
            station_call: g!(),
            operator_call: g!(),
            qso_start_utc: g!(),
            qso_end_utc: g!(),
            frequency_hz: g!(),
            band: g!(),
            mode: g!(),
            submode: g!(),
            rst_sent: g!(),
            rst_rcvd: g!(),
            my_grid: g!(),
            their_grid: g!(),
            qso_notes: g!(),
            my_pota_ref: g!(),
            pota_ref: g!(),
            my_sota_ref: g!(),
            sota_ref: g!(),
        })
    } else {
        c += 18;
        None
    };
    let em_id: Option<String> = g!();
    let emergency = if em_id.is_some() {
        Some(EmergencyFields {
            record_type: enum_col(g!(), EmergencyRecordType::parse)?,
            incident_name: g!(),
            incident_number: g!(),
            operational_period: g!(),
            message_number: g!(),
            precedence: g!(),
            originator: g!(),
            originator_position: g!(),
            recipient: g!(),
            recipient_position: g!(),
            sender: g!(),
            receiver: g!(),
            sent_at_utc: g!(),
            received_at_utc: g!(),
            reply_requested: g!(),
            action_requested: g!(),
            response: g!(),
            resource_unit: g!(),
            assignment: g!(),
            activity_start_utc: g!(),
            activity_end_utc: g!(),
            location: g!(),
            disposition: g!(),
            preparer: g!(),
            approved_by_name: g!(),
            approved_by_position: g!(),
            replied_by_name: g!(),
            replied_by_position: g!(),
            replied_at_utc: g!(),
        })
    } else {
        None
    };
    let _ = c;
    Ok(Entry {
        id,
        sequence_no,
        kind,
        event_at_utc,
        event_timezone,
        event_utc_offset_min,
        recorded_at_utc,
        updated_at_utc,
        status,
        voided_at_utc,
        void_reason,
        direction,
        priority,
        handling: handling.as_deref().and_then(Handling::parse),
        flagged,
        from_text,
        to_text,
        subject,
        body,
        channel,
        location,
        tags: serde_json::from_str(&tags_json).unwrap_or_default(),
        operator_snapshot: serde_json::from_str(&op_json).unwrap_or_default(),
        current_revision,
        favorite,
        ham,
        emergency,
        attachments: vec![],
        op_id,
        station_snapshot: serde_json::from_str(&station_json).unwrap_or_default(),
        operator_location: op_loc_json.and_then(|j| serde_json::from_str(&j).ok()),
        reported_location: rep_loc_json.and_then(|j| serde_json::from_str(&j).ok()),
        important,
        follow_up,
        related_count,
    })
}

/// Insert one new entry (row, HAM/emergency parts, attachments, numbering, search index, first revision)
/// inside the caller's transaction. Used by Save Entry and by ADIF import (many entries, one transaction).
/// `station`: the station snapshot to record; None = the logbook's current station.
#[allow(clippy::too_many_arguments)]
pub(crate) fn insert_new_entry(
    tx: &rusqlite::Transaction,
    logbook_id: &str,
    v: &EntryInput,
    finals: &[FinalizedAttachment],
    operator: &OperatorSnapshot,
    request_id: &str,
    station: Option<&crate::models::StationSnapshot>,
) -> AppResult<Entry> {
    let seq: i64 = tx.query_row("SELECT next_sequence FROM logbook", [], |r| r.get(0))?;
    let id = fsutil::new_uuid();
    let now = now_utc();
    tx.execute(
        "INSERT INTO entries(id, logbook_id, sequence_no, kind, event_at_utc, event_timezone, event_utc_offset_min,
            recorded_at_utc, updated_at_utc, status, direction, priority, handling, flagged, from_text, to_text, subject,
            body, channel, location, operator_snapshot_json, tags_json, current_revision, client_request_id, op_id,
            station_snapshot_json, operator_location_json, reported_location_json)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8,'active',?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,1,?21,?22,?23,?24,?25)",
        params![
            id,
            logbook_id,
            seq,
            v.kind.as_str(),
            v.event_at_utc,
            v.event_timezone,
            v.event_utc_offset_min,
            now,
            v.direction.as_str(),
            v.priority.as_str(),
            v.handling.map(|h| h.as_str()),
            v.flagged,
            v.from_text,
            v.to_text,
            v.subject,
            v.body,
            v.channel,
            v.location,
            serde_json::to_string(operator)?,
            serde_json::to_string(&v.tags)?,
            request_id,
            v.op_id,
            match station {
                Some(st) => serde_json::to_string(st)?,
                None => serde_json::to_string(&crate::station::read_station(tx)?.snapshot())?,
            },
            location_json(&v.operator_location)?,
            location_json(&v.reported_location)?
        ],
    )?;
    write_flags(tx, &id, v, None, &now)?;
    if let Some(h) = &v.ham {
        insert_ham(tx, &id, h)?;
    }
    if let Some(m) = &v.emergency {
        insert_emergency(tx, &id, m)?;
    }
    for f in finals {
        insert_attachment(tx, &id, f, 1)?;
    }
    tx.execute("UPDATE logbook SET next_sequence = ?1, updated_at_utc = ?2", params![seq + 1, now])?;
    write_fts(tx, &id)?;
    let entry = load_entry(tx, &id)?;
    insert_revision(tx, &id, 1, "create", "", None, &entry, operator, request_id)?;
    Ok(entry)
}

/// Important + follow-up columns (schema v5). `resolved_utc` is set by the core when an entry becomes
/// resolved (kept if it was already resolved) and cleared when it is reopened or cleared.
fn write_flags(tx: &rusqlite::Transaction, id: &str, v: &EntryInput, before: Option<&FollowUp>, now: &str) -> AppResult<()> {
    let f = &v.follow_up;
    let resolved = match f.state {
        FollowUpState::Resolved => match before {
            Some(b) if b.state == FollowUpState::Resolved && b.resolved_utc.is_some() => b.resolved_utc.clone(),
            _ => Some(now.to_string()),
        },
        _ => None,
    };
    let (due, note) = match f.state {
        FollowUpState::None => (None, String::new()),
        _ => (f.due_utc.clone(), f.note.clone()),
    };
    tx.execute(
        "UPDATE entries SET important=?2, follow_up_state=?3, follow_up_due_utc=?4, follow_up_note=?5, follow_up_resolved_utc=?6
         WHERE id=?1",
        params![id, v.important, f.state.as_str(), due, note, resolved],
    )?;
    Ok(())
}

fn location_json(l: &Option<Location>) -> AppResult<Option<String>> {
    Ok(match l {
        Some(l) => Some(serde_json::to_string(l)?),
        None => None,
    })
}

pub fn load_attachments(conn: &Connection, entry_id: &str) -> AppResult<Vec<Attachment>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, rel_path, display_name, mime, size_bytes, sha256, added_at_utc, added_in_revision, caption, source
         FROM attachments WHERE entry_id=?1 AND removed_in_revision IS NULL ORDER BY added_at_utc, display_name, id",
    )?;
    let rows = stmt.query_map(params![entry_id], |r| {
        Ok(Attachment {
            id: r.get(0)?,
            rel_path: r.get(1)?,
            display_name: r.get(2)?,
            mime: r.get(3)?,
            size_bytes: r.get(4)?,
            sha256: r.get(5)?,
            added_at_utc: r.get(6)?,
            added_in_revision: r.get(7)?,
            caption: r.get(8)?,
            source: r.get(9)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

pub fn load_entry(conn: &Connection, id: &str) -> AppResult<Entry> {
    let sql = format!("{ENTRY_SELECT} WHERE e.id = ?1");
    let mut e = conn.query_row(&sql, params![id], row_to_entry).optional()?.ok_or_else(|| AppError::not_found("Entry not found"))?;
    e.attachments = load_attachments(conn, id)?;
    Ok(e)
}

/// Canonical revision snapshot: the full entry minus the favorite marker (not part of history).
pub fn snapshot(e: &Entry) -> serde_json::Value {
    let mut v = serde_json::to_value(e).unwrap_or(serde_json::Value::Null);
    if let Some(o) = v.as_object_mut() {
        o.remove("favorite");
        // Links live in their own audit trail; adding one is not an edit of the entry.
        o.remove("related_count");
    }
    v
}

fn content_view(e: &Entry) -> serde_json::Value {
    let mut v = snapshot(e);
    if let Some(o) = v.as_object_mut() {
        o.remove("updated_at_utc");
        o.remove("current_revision");
    }
    v
}

fn check_request_id(id: &str) -> AppResult<()> {
    if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(AppError::validation("client_request_id", "Invalid request id"));
    }
    Ok(())
}

fn insert_ham(c: &Connection, id: &str, h: &HamFields) -> AppResult<()> {
    c.execute(
        "INSERT INTO entry_ham(entry_id, their_call, station_call, operator_call, qso_start_utc, qso_end_utc, frequency_hz, band,
            mode, submode, rst_sent, rst_rcvd, my_grid, their_grid, qso_notes, my_pota_ref, pota_ref, my_sota_ref, sota_ref)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
        params![
            id,
            h.their_call,
            h.station_call,
            h.operator_call,
            h.qso_start_utc,
            h.qso_end_utc,
            h.frequency_hz,
            h.band,
            h.mode,
            h.submode,
            h.rst_sent,
            h.rst_rcvd,
            h.my_grid,
            h.their_grid,
            h.qso_notes,
            h.my_pota_ref,
            h.pota_ref,
            h.my_sota_ref,
            h.sota_ref
        ],
    )?;
    Ok(())
}

fn insert_emergency(c: &Connection, id: &str, m: &EmergencyFields) -> AppResult<()> {
    c.execute(
        "INSERT INTO entry_emergency(entry_id, record_type, incident_name, incident_number, operational_period, message_number,
            precedence, originator, originator_position, recipient, recipient_position, sender, receiver, sent_at_utc,
            received_at_utc, reply_requested, action_requested, response, resource_unit, assignment, activity_start_utc,
            activity_end_utc, location, disposition, preparer, approved_by_name, approved_by_position, replied_by_name,
            replied_by_position, replied_at_utc)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30)",
        params![
            id,
            m.record_type.as_str(),
            m.incident_name,
            m.incident_number,
            m.operational_period,
            m.message_number,
            m.precedence,
            m.originator,
            m.originator_position,
            m.recipient,
            m.recipient_position,
            m.sender,
            m.receiver,
            m.sent_at_utc,
            m.received_at_utc,
            m.reply_requested,
            m.action_requested,
            m.response,
            m.resource_unit,
            m.assignment,
            m.activity_start_utc,
            m.activity_end_utc,
            m.location,
            m.disposition,
            m.preparer,
            m.approved_by_name,
            m.approved_by_position,
            m.replied_by_name,
            m.replied_by_position,
            m.replied_at_utc
        ],
    )?;
    Ok(())
}

fn write_fts(c: &Connection, id: &str) -> AppResult<()> {
    let rid: i64 = c.query_row("SELECT rid FROM entries WHERE id=?1", params![id], |r| r.get(0))?;
    let e = load_entry(c, id)?;
    c.execute("DELETE FROM entries_fts WHERE rowid=?1", params![rid])?;
    let mut parties = vec![e.from_text.clone(), e.to_text.clone()];
    let mut extra = vec![e.channel.clone(), e.location.clone()];
    if let Some(h) = &e.ham {
        parties.extend([h.their_call.clone(), h.station_call.clone(), h.operator_call.clone()]);
        extra.extend([h.qso_notes.clone(), h.mode.clone(), h.band.clone(), h.their_grid.clone()]);
    }
    if let Some(m) = &e.emergency {
        parties.extend([
            m.originator.clone(),
            m.recipient.clone(),
            m.sender.clone(),
            m.receiver.clone(),
            m.resource_unit.clone(),
            m.preparer.clone(),
            m.approved_by_name.clone(),
            m.replied_by_name.clone(),
        ]);
        extra.extend([
            m.message_number.clone(),
            m.action_requested.clone(),
            m.response.clone(),
            m.assignment.clone(),
            m.disposition.clone(),
            m.incident_name.clone(),
            m.location.clone(),
        ]);
    }
    for a in &e.attachments {
        extra.push(a.display_name.clone());
    }
    c.execute(
        "INSERT INTO entries_fts(rowid, subject, body, parties, tags, extra) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![rid, e.subject, e.body, parties.join(" "), e.tags.join(" "), extra.join(" ")],
    )?;
    Ok(())
}

fn guess_mime(name: &str) -> &'static str {
    match fsutil::safe_extension(name).as_str() {
        "txt" | "log" => "text/plain",
        "csv" => "text/csv",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "json" => "application/json",
        "adi" => "text/plain",
        _ => "application/octet-stream",
    }
}

pub(crate) struct FinalizedAttachment {
    rel_path: String,
    display_name: String,
    size: u64,
    sha256: String,
    caption: String,
    source: String,
    mime: String,
}

/// Copy a staged file into the live attachments directory (content addressed), verifying its hash.
/// Newly created files are recorded so a failed commit can remove them.
fn finalize_attachment(dir: &Path, staged: &StagedAttachment, created: &mut Vec<PathBuf>) -> AppResult<FinalizedAttachment> {
    let staging = dir.join(STAGING_DIR);
    let data = staging.join(&staged.staging_id);
    let meta: StagedAttachment =
        fs::read(staging.join(format!("{}.json", staged.staging_id))).ok().and_then(|b| serde_json::from_slice(&b).ok()).ok_or_else(
            || AppError::validation("attachments", format!("Attachment '{}' is no longer staged; attach it again", staged.display_name)),
        )?;
    if !data.is_file() {
        return Err(AppError::validation(
            "attachments",
            format!("Attachment '{}' is no longer staged; attach it again", staged.display_name),
        ));
    }
    let ext = fsutil::safe_extension(&meta.display_name);
    let fname = if ext.is_empty() { meta.sha256.clone() } else { format!("{}.{ext}", meta.sha256) };
    let rel_path = format!("{ATTACHMENTS_DIR}/{fname}");
    let final_path = dir.join(&rel_path);
    let att_dir = dir.join(ATTACHMENTS_DIR);
    let existing_ok = final_path.is_file() && fsutil::sha256_file(&final_path).map(|(h, _)| h == meta.sha256).unwrap_or(false);
    if !existing_ok {
        let tmp = att_dir.join(format!(".tmp-{}", fsutil::new_uuid()));
        let copied = fsutil::copy_synced(&data, &tmp);
        let (h, size) = match copied {
            Ok(v) => v,
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                return Err(e.with_context("Could not copy attachment into the logbook"));
            }
        };
        if h != meta.sha256 || size != meta.size_bytes {
            let _ = fs::remove_file(&tmp);
            return Err(AppError::new(ErrorCode::Integrity, format!("Attachment '{}' changed after it was staged", meta.display_name)));
        }
        let was_present = final_path.exists();
        fs::rename(&tmp, &final_path).inspect_err(|_| {
            let _ = fs::remove_file(&tmp);
        })?;
        fsutil::sync_dir(&att_dir)?;
        if !was_present {
            created.push(final_path);
        }
    }
    Ok(FinalizedAttachment {
        rel_path,
        display_name: if staged.display_name.is_empty() { meta.display_name.clone() } else { staged.display_name.clone() },
        size: meta.size_bytes,
        sha256: meta.sha256,
        caption: staged.caption.clone(),
        // The source and detected type come from staging (the core's record), not from the request.
        source: if crate::attach::SOURCES.contains(&meta.source.as_str()) { meta.source } else { "file".into() },
        mime: if meta.mime.is_empty() { guess_mime(&meta.display_name).to_string() } else { meta.mime },
    })
}

fn insert_attachment(c: &Connection, entry_id: &str, f: &FinalizedAttachment, revision: i64) -> AppResult<()> {
    c.execute(
        "INSERT INTO attachments(id, entry_id, rel_path, display_name, mime, size_bytes, sha256, added_at_utc, added_in_revision,
            caption, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            fsutil::new_uuid(),
            entry_id,
            f.rel_path,
            f.display_name,
            f.mime,
            f.size as i64,
            f.sha256,
            now_utc(),
            revision,
            f.caption,
            f.source
        ],
    )?;
    Ok(())
}

/// An entry may only be newly assigned to an existing, active operational period. Keeping the period
/// it already has is always allowed (it may have been voided since).
fn check_op(c: &Connection, op_id: &Option<String>, current: Option<&str>) -> AppResult<()> {
    let Some(id) = op_id else { return Ok(()) };
    if current == Some(id.as_str()) {
        return Ok(());
    }
    let status: Option<String> = c.query_row("SELECT status FROM operational_periods WHERE id=?1", params![id], |r| r.get(0)).optional()?;
    match status.as_deref() {
        Some("active") => Ok(()),
        Some(_) => Err(AppError::validation("op_id", "That operational period is voided")),
        None => Err(AppError::validation("op_id", "That operational period does not exist")),
    }
}

fn fill_incident_snapshot(c: &Connection, m: &mut EmergencyFields) -> AppResult<()> {
    let (n, num, op): (String, String, String) =
        c.query_row("SELECT incident_name, incident_number, operational_period FROM logbook", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
    if m.incident_name.is_empty() {
        m.incident_name = n;
    }
    if m.incident_number.is_empty() {
        m.incident_number = num;
    }
    if m.operational_period.is_empty() {
        m.operational_period = op;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_revision(
    c: &Connection,
    entry_id: &str,
    revision: i64,
    action: &str,
    reason: &str,
    before: Option<&Entry>,
    after: &Entry,
    editor: &OperatorSnapshot,
    request_id: &str,
) -> AppResult<()> {
    c.execute(
        "INSERT INTO entry_revisions(entry_id, revision, action, at_utc, reason, before_json, after_json, editor_snapshot_json, client_request_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            entry_id,
            revision,
            action,
            now_utc(),
            reason,
            before.map(|b| snapshot(b).to_string()),
            snapshot(after).to_string(),
            serde_json::to_string(editor)?,
            request_id
        ],
    )?;
    Ok(())
}

fn clean_reason(reason: &str) -> AppResult<String> {
    let r = reason.trim();
    if r.chars().count() > 500 {
        return Err(AppError::validation("reason", "Reason must be at most 500 characters"));
    }
    Ok(r.to_string())
}

impl Logbook {
    /// Save Entry: allocate the next sequence number, insert the entry and its first revision,
    /// and commit in one IMMEDIATE transaction. Retrying with the same request id returns the
    /// already-committed entry instead of creating a duplicate.
    pub fn save_new(&mut self, input: &EntryInput, request_id: &str, operator: &OperatorSnapshot) -> AppResult<SaveResult> {
        check_request_id(request_id)?;
        self.ensure_writable()?;
        let mut v = validate_entry_input(input, self.logbook_type, chrono::Utc::now())?;
        if !v.remove_attachments.is_empty() || !v.attachment_captions.is_empty() {
            return Err(AppError::validation("attachments", "A new entry has no saved attachments to change"));
        }
        let dir = self.dir.clone();
        let logbook_id = self.id.clone();
        let mut created: Vec<PathBuf> = Vec::new();
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<String> =
            tx.query_row("SELECT id FROM entries WHERE client_request_id=?1", params![request_id], |r| r.get(0)).optional()?;
        if let Some(id) = existing {
            let entry = load_entry(&tx, &id)?;
            return Ok(SaveResult { entry, duplicate: true, unchanged: false });
        }
        let used_by_revision: Option<i64> =
            tx.query_row("SELECT 1 FROM entry_revisions WHERE client_request_id=?1", params![request_id], |r| r.get(0)).optional()?;
        if used_by_revision.is_some() {
            return Err(AppError::new(ErrorCode::Conflict, "Request id already used by a different change"));
        }
        let result = (|| -> AppResult<Entry> {
            check_op(&tx, &v.op_id, None)?;
            if let Some(m) = v.emergency.as_mut() {
                fill_incident_snapshot(&tx, m)?;
            }
            let mut finals = Vec::new();
            for s in &v.new_attachments {
                finals.push(finalize_attachment(&dir, s, &mut created)?);
            }
            let entry = insert_new_entry(&tx, &logbook_id, &v, &finals, operator, request_id, None)?;
            faults::check("entry.save.before_commit")?;
            Ok(entry)
        })();
        match result {
            Ok(entry) => {
                if let Err(e) = tx.commit() {
                    for p in &created {
                        let _ = fs::remove_file(p);
                    }
                    return Err(AppError::from(e));
                }
                for s in &v.new_attachments {
                    self.discard_staged(&s.staging_id);
                }
                Ok(SaveResult { entry, duplicate: false, unchanged: false })
            }
            Err(e) => {
                drop(tx);
                for p in &created {
                    let _ = fs::remove_file(p);
                }
                Err(e)
            }
        }
    }

    /// Save Changes: same entry id and sequence number; before/after revision in one transaction.
    pub fn save_changes(
        &mut self,
        entry_id: &str,
        base_revision: i64,
        input: &EntryInput,
        reason: &str,
        request_id: &str,
        editor: &OperatorSnapshot,
    ) -> AppResult<SaveResult> {
        check_request_id(request_id)?;
        self.ensure_writable()?;
        let reason = clean_reason(reason)?;
        let mut v = validate_entry_input(input, self.logbook_type, chrono::Utc::now())?;
        let dir = self.dir.clone();
        let mut created: Vec<PathBuf> = Vec::new();
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(prev) = tx
            .query_row("SELECT entry_id FROM entry_revisions WHERE client_request_id=?1", params![request_id], |r| r.get::<_, String>(0))
            .optional()?
        {
            if prev != entry_id {
                return Err(AppError::new(ErrorCode::Conflict, "Request id already used by a different entry"));
            }
            let entry = load_entry(&tx, entry_id)?;
            return Ok(SaveResult { entry, duplicate: true, unchanged: false });
        }
        let before = load_entry(&tx, entry_id)?;
        if before.status == EntryStatus::Void {
            return Err(AppError::validation("status", "This entry is voided. Restore it before editing."));
        }
        if before.current_revision != base_revision {
            return Err(AppError::new(
                ErrorCode::Conflict,
                format!(
                    "Entry #{} changed since you opened it (now revision {}). Reload it before saving.",
                    before.sequence_no, before.current_revision
                ),
            ));
        }
        let new_rev = before.current_revision + 1;
        let result = (|| -> AppResult<Option<Entry>> {
            check_op(&tx, &v.op_id, before.op_id.as_deref())?;
            if let Some(m) = v.emergency.as_mut() {
                fill_incident_snapshot(&tx, m)?;
            }
            let mut finals = Vec::new();
            for s in &v.new_attachments {
                finals.push(finalize_attachment(&dir, s, &mut created)?);
            }
            let now = now_utc();
            tx.execute(
                "UPDATE entries SET kind=?2, event_at_utc=?3, event_timezone=?4, event_utc_offset_min=?5, updated_at_utc=?6,
                    direction=?7, priority=?8, handling=?9, flagged=?10, from_text=?11, to_text=?12, subject=?13, body=?14,
                    channel=?15, location=?16, tags_json=?17, current_revision=?18, op_id=?19,
                    operator_location_json=?20, reported_location_json=?21
                 WHERE id=?1",
                params![
                    entry_id,
                    v.kind.as_str(),
                    v.event_at_utc,
                    v.event_timezone,
                    v.event_utc_offset_min,
                    now,
                    v.direction.as_str(),
                    v.priority.as_str(),
                    v.handling.map(|h| h.as_str()),
                    v.flagged,
                    v.from_text,
                    v.to_text,
                    v.subject,
                    v.body,
                    v.channel,
                    v.location,
                    serde_json::to_string(&v.tags)?,
                    new_rev,
                    v.op_id,
                    location_json(&v.operator_location)?,
                    location_json(&v.reported_location)?
                ],
            )?;
            write_flags(&tx, entry_id, &v, Some(&before.follow_up), &now)?;
            tx.execute("DELETE FROM entry_ham WHERE entry_id=?1", params![entry_id])?;
            tx.execute("DELETE FROM entry_emergency WHERE entry_id=?1", params![entry_id])?;
            if let Some(h) = &v.ham {
                insert_ham(&tx, entry_id, h)?;
            }
            if let Some(m) = &v.emergency {
                insert_emergency(&tx, entry_id, m)?;
            }
            for f in &finals {
                insert_attachment(&tx, entry_id, f, new_rev)?;
            }
            // 2.2: removals and caption changes are part of this revision. Rows and files stay (history,
            // backups); a removed attachment simply no longer belongs to the current entry.
            for id in &v.remove_attachments {
                let n = tx.execute(
                    "UPDATE attachments SET removed_in_revision=?3 WHERE id=?1 AND entry_id=?2 AND removed_in_revision IS NULL",
                    params![id, entry_id, new_rev],
                )?;
                if n == 0 {
                    return Err(AppError::validation("attachments", "That attachment is not on this entry"));
                }
            }
            for c in &v.attachment_captions {
                let n = tx.execute(
                    "UPDATE attachments SET caption=?3 WHERE id=?1 AND entry_id=?2 AND removed_in_revision IS NULL",
                    params![c.id, entry_id, c.caption],
                )?;
                if n == 0 {
                    return Err(AppError::validation("attachments", "That attachment is not on this entry"));
                }
            }
            let after = load_entry(&tx, entry_id)?;
            if content_view(&after) == content_view(&before) {
                return Ok(None);
            }
            write_fts(&tx, entry_id)?;
            tx.execute("UPDATE logbook SET updated_at_utc=?1", params![now])?;
            insert_revision(&tx, entry_id, new_rev, "edit", &reason, Some(&before), &after, editor, request_id)?;
            faults::check("entry.update.before_commit")?;
            Ok(Some(after))
        })();
        match result {
            Ok(Some(entry)) => {
                if let Err(e) = tx.commit() {
                    for p in &created {
                        let _ = fs::remove_file(p);
                    }
                    return Err(AppError::from(e));
                }
                for s in &v.new_attachments {
                    self.discard_staged(&s.staging_id);
                }
                Ok(SaveResult { entry, duplicate: false, unchanged: false })
            }
            Ok(None) => {
                drop(tx);
                Ok(SaveResult { entry: before, duplicate: false, unchanged: true })
            }
            Err(e) => {
                drop(tx);
                for p in &created {
                    let _ = fs::remove_file(p);
                }
                Err(e)
            }
        }
    }

    fn change_status(
        &mut self,
        entry_id: &str,
        to_void: bool,
        reason: &str,
        request_id: &str,
        editor: &OperatorSnapshot,
    ) -> AppResult<SaveResult> {
        check_request_id(request_id)?;
        self.ensure_writable()?;
        let reason = clean_reason(reason)?;
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(prev) = tx
            .query_row("SELECT entry_id FROM entry_revisions WHERE client_request_id=?1", params![request_id], |r| r.get::<_, String>(0))
            .optional()?
        {
            if prev != entry_id {
                return Err(AppError::new(ErrorCode::Conflict, "Request id already used by a different entry"));
            }
            let entry = load_entry(&tx, entry_id)?;
            return Ok(SaveResult { entry, duplicate: true, unchanged: false });
        }
        let before = load_entry(&tx, entry_id)?;
        let want = if to_void { EntryStatus::Void } else { EntryStatus::Active };
        if before.status == want {
            return Ok(SaveResult { entry: before, duplicate: false, unchanged: true });
        }
        let now = now_utc();
        let new_rev = before.current_revision + 1;
        if to_void {
            tx.execute(
                "UPDATE entries SET status='void', voided_at_utc=?2, void_reason=?3, updated_at_utc=?2, current_revision=?4 WHERE id=?1",
                params![entry_id, now, reason, new_rev],
            )?;
        } else {
            tx.execute(
                "UPDATE entries SET status='active', voided_at_utc=NULL, void_reason=NULL, updated_at_utc=?2, current_revision=?3 WHERE id=?1",
                params![entry_id, now, new_rev],
            )?;
        }
        tx.execute("UPDATE logbook SET updated_at_utc=?1", params![now])?;
        let after = load_entry(&tx, entry_id)?;
        insert_revision(
            &tx,
            entry_id,
            new_rev,
            if to_void { "void" } else { "restore" },
            &reason,
            Some(&before),
            &after,
            editor,
            request_id,
        )?;
        faults::check("entry.status.before_commit")?;
        tx.commit()?;
        Ok(SaveResult { entry: after, duplicate: false, unchanged: false })
    }

    /// Void is recoverable: the number is never reused and history is kept.
    pub fn void_entry(&mut self, entry_id: &str, reason: &str, request_id: &str, editor: &OperatorSnapshot) -> AppResult<SaveResult> {
        self.change_status(entry_id, true, reason, request_id, editor)
    }

    pub fn restore_entry(&mut self, entry_id: &str, reason: &str, request_id: &str, editor: &OperatorSnapshot) -> AppResult<SaveResult> {
        self.change_status(entry_id, false, reason, request_id, editor)
    }

    pub fn get_entry(&self, id: &str) -> AppResult<Entry> {
        load_entry(&self.conn, id)
    }

    pub fn revisions(&self, entry_id: &str) -> AppResult<Vec<EntryRevision>> {
        list_revisions(&self.conn, entry_id)
    }

    pub fn set_favorite(&mut self, entry_id: &str, favorite: bool) -> AppResult<Entry> {
        self.ensure_writable()?;
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        load_entry(&tx, entry_id)?;
        if favorite {
            tx.execute("INSERT OR IGNORE INTO entry_favorites(entry_id, created_at_utc) VALUES (?1, ?2)", params![entry_id, now_utc()])?;
        } else {
            tx.execute("DELETE FROM entry_favorites WHERE entry_id=?1", params![entry_id])?;
        }
        tx.commit()?;
        load_entry(&self.conn, entry_id)
    }

    pub fn list_entries(&self, q: &EntryQuery) -> AppResult<EntryPage> {
        list_entries(&self.conn, q)
    }

    // ---------- templates ----------

    pub fn templates(&self) -> AppResult<Vec<EntryTemplate>> {
        list_templates(&self.conn)
    }

    pub fn save_template(&mut self, id: Option<&str>, name: &str, defaults: &TemplateDefaults) -> AppResult<EntryTemplate> {
        self.ensure_writable()?;
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 80 {
            return Err(AppError::validation("name", "Template name must be 1-80 characters"));
        }
        let d = validate_template_defaults(defaults, self.logbook_type)?;
        let now = now_utc();
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let dup: Option<String> =
            tx.query_row("SELECT id FROM entry_templates WHERE name=?1 COLLATE NOCASE", params![name], |r| r.get(0)).optional()?;
        let tid = match id {
            Some(existing) => {
                if dup.as_deref().is_some_and(|d| d != existing) {
                    return Err(AppError::new(ErrorCode::AlreadyExists, "Another template already has this name"));
                }
                let n = tx.execute(
                    "UPDATE entry_templates SET name=?2, kind=?3, version=version+1, defaults_json=?4, updated_at_utc=?5 WHERE id=?1",
                    params![existing, name, d.kind.map(|k| k.as_str()).unwrap_or(""), serde_json::to_string(&d)?, now],
                )?;
                if n == 0 {
                    return Err(AppError::not_found("Template not found"));
                }
                existing.to_string()
            }
            None => {
                if dup.is_some() {
                    return Err(AppError::new(ErrorCode::AlreadyExists, "A template with this name already exists"));
                }
                let tid = fsutil::new_uuid();
                tx.execute(
                    "INSERT INTO entry_templates(id, logbook_id, name, kind, version, defaults_json, created_at_utc, updated_at_utc)
                     VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?6)",
                    params![tid, self.id, name, d.kind.map(|k| k.as_str()).unwrap_or(""), serde_json::to_string(&d)?, now],
                )?;
                tid
            }
        };
        tx.commit()?;
        list_templates(&self.conn)?.into_iter().find(|t| t.id == tid).ok_or_else(|| AppError::not_found("Template not found"))
    }

    pub fn delete_template(&mut self, id: &str) -> AppResult<()> {
        self.ensure_writable()?;
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM entry_templates WHERE id=?1", params![id])?;
        tx.commit()?;
        Ok(())
    }
}

pub fn list_revisions(conn: &Connection, entry_id: &str) -> AppResult<Vec<EntryRevision>> {
    let mut stmt = conn.prepare(
        "SELECT revision, action, at_utc, reason, before_json, after_json, editor_snapshot_json
         FROM entry_revisions WHERE entry_id=?1 ORDER BY revision",
    )?;
    let rows = stmt.query_map(params![entry_id], |r| {
        Ok(EntryRevision {
            revision: r.get(0)?,
            action: r.get(1)?,
            at_utc: r.get(2)?,
            reason: r.get(3)?,
            before: r.get::<_, Option<String>>(4)?.and_then(|s| serde_json::from_str(&s).ok()),
            after: serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or(serde_json::Value::Null),
            editor: serde_json::from_str(&r.get::<_, String>(6)?).unwrap_or_default(),
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

pub fn list_templates(conn: &Connection) -> AppResult<Vec<EntryTemplate>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, kind, version, defaults_json, created_at_utc, updated_at_utc FROM entry_templates ORDER BY name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(EntryTemplate {
            id: r.get(0)?,
            name: r.get(1)?,
            kind: r.get(2)?,
            version: r.get(3)?,
            defaults: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or_default(),
            created_at_utc: r.get(5)?,
            updated_at_utc: r.get(6)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

/// Turn free text into an FTS5 query: each whitespace token becomes a quoted prefix term (AND).
pub fn fts_query(text: &str) -> Option<String> {
    let terms: Vec<String> =
        text.split_whitespace().map(|t| t.replace('"', "")).filter(|t| !t.is_empty()).map(|t| format!("\"{t}\"*")).collect();
    if terms.is_empty() { None } else { Some(terms.join(" ")) }
}

fn like_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('%');
    for c in s.chars() {
        if c == '%' || c == '_' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

/// Build the WHERE clause and parameters for a query. Shared by the Traffic Log and exports so
/// "current filter" exports match exactly what the operator sees.
pub fn build_filter(q: &EntryQuery) -> AppResult<(String, Vec<SqlValue>)> {
    let mut clauses: Vec<String> = Vec::new();
    let mut p: Vec<SqlValue> = Vec::new();
    match q.status.as_str() {
        "" | "active" => clauses.push("e.status = 'active'".into()),
        "void" => clauses.push("e.status = 'void'".into()),
        "all" => {}
        other => return Err(AppError::validation("status", format!("Unknown status filter '{other}'"))),
    }
    if let Some(m) = fts_query(&q.text) {
        clauses.push("e.rid IN (SELECT rowid FROM entries_fts WHERE entries_fts MATCH ?)".into());
        p.push(SqlValue::Text(m));
    }
    if let Some(f) = &q.from_utc {
        clauses.push("e.event_at_utc >= ?".into());
        p.push(SqlValue::Text(crate::validate::normalize_utc("from_utc", f)?));
    }
    if let Some(t) = &q.to_utc {
        clauses.push("e.event_at_utc < ?".into());
        p.push(SqlValue::Text(crate::validate::normalize_utc("to_utc", t)?));
    }
    let mut in_list = |col: &str, vals: Vec<&'static str>| {
        if !vals.is_empty() {
            clauses.push(format!("{col} IN ({})", vec!["?"; vals.len()].join(",")));
            p.extend(vals.into_iter().map(|v| SqlValue::Text(v.to_string())));
        }
    };
    in_list("e.kind", q.kinds.iter().map(|k| k.as_str()).collect());
    in_list("e.direction", q.directions.iter().map(|k| k.as_str()).collect());
    in_list("e.priority", q.priorities.iter().map(|k| k.as_str()).collect());
    in_list("e.handling", q.handling.iter().map(|k| k.as_str()).collect());
    let station = q.station.trim();
    if !station.is_empty() {
        clauses.push(
            "(e.from_text LIKE ? ESCAPE '\\' OR e.to_text LIKE ? ESCAPE '\\' OR h.their_call LIKE ? ESCAPE '\\'
              OR h.station_call LIKE ? ESCAPE '\\' OR m.originator LIKE ? ESCAPE '\\' OR m.recipient LIKE ? ESCAPE '\\')"
                .into(),
        );
        let pat = like_escape(station);
        for _ in 0..6 {
            p.push(SqlValue::Text(pat.clone()));
        }
    }
    let operator = q.operator.trim();
    if !operator.is_empty() {
        // A profile id, or the snapshot name for entries whose profile is not on this computer.
        clauses.push(
            "(json_extract(e.operator_snapshot_json, '$.profile_id') = ? OR json_extract(e.operator_snapshot_json, '$.name') = ?)".into(),
        );
        p.push(SqlValue::Text(operator.to_string()));
        p.push(SqlValue::Text(operator.to_string()));
    }
    for t in &q.tags {
        let t = t.trim();
        if !t.is_empty() {
            clauses.push("EXISTS (SELECT 1 FROM json_each(e.tags_json) WHERE upper(json_each.value) = upper(?))".into());
            p.push(SqlValue::Text(t.to_string()));
        }
    }
    if q.favorites_only {
        clauses.push("f.entry_id IS NOT NULL".into());
    }
    if q.important_only {
        clauses.push("e.important = 1".into());
    }
    match q.follow_up.as_str() {
        "open" => clauses.push("e.follow_up_state = 'open'".into()),
        "resolved" => clauses.push("e.follow_up_state = 'resolved'".into()),
        "any" => clauses.push("e.follow_up_state <> 'none'".into()),
        _ => {}
    }
    if q.flagged_only {
        clauses.push("e.flagged = 1".into());
    }
    if !q.ids.is_empty() {
        if q.ids.len() > 10_000 {
            return Err(AppError::validation("ids", "Too many entries selected"));
        }
        clauses.push(format!("e.id IN ({})", vec!["?"; q.ids.len()].join(",")));
        p.extend(q.ids.iter().map(|i| SqlValue::Text(i.clone())));
    }
    let w = if clauses.is_empty() { String::new() } else { format!(" WHERE {}", clauses.join(" AND ")) };
    Ok((w, p))
}

pub fn order_clause(q: &EntryQuery) -> &'static str {
    match (q.sort.as_str(), q.descending) {
        ("recorded", false) => " ORDER BY e.recorded_at_utc ASC, e.sequence_no ASC",
        ("recorded", true) => " ORDER BY e.recorded_at_utc DESC, e.sequence_no DESC",
        (_, false) => " ORDER BY e.event_at_utc ASC, e.sequence_no ASC",
        (_, true) => " ORDER BY e.event_at_utc DESC, e.sequence_no DESC",
    }
}

pub fn count_entries(conn: &Connection, q: &EntryQuery) -> AppResult<i64> {
    let (w, p) = build_filter(q)?;
    let sql = format!(
        "SELECT COUNT(*) FROM entries e LEFT JOIN entry_favorites f ON f.entry_id=e.id
         LEFT JOIN entry_ham h ON h.entry_id=e.id LEFT JOIN entry_emergency m ON m.entry_id=e.id{w}"
    );
    Ok(conn.query_row(&sql, params_from_iter(p.iter()), |r| r.get(0))?)
}

pub fn list_entries(conn: &Connection, q: &EntryQuery) -> AppResult<EntryPage> {
    let (w, p) = build_filter(q)?;
    let total = count_entries(conn, q)?;
    let limit = if q.limit == 0 { 100 } else { q.limit.min(MAX_PAGE) };
    let sql = format!("{ENTRY_SELECT}{w}{} LIMIT {limit} OFFSET {}", order_clause(q), q.offset);
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(p.iter()), row_to_entry)?;
    let mut items = Vec::new();
    for r in rows {
        let mut e = r?;
        e.attachments = load_attachments(conn, &e.id)?;
        items.push(e);
    }
    Ok(EntryPage { total, items })
}

/// Stream every matching entry in order without materializing the whole set.
pub fn for_each_entry(conn: &Connection, q: &EntryQuery, mut f: impl FnMut(&Entry) -> AppResult<()>) -> AppResult<()> {
    let (w, p) = build_filter(q)?;
    let sql = format!("{ENTRY_SELECT}{w}{}", order_clause(q));
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(p.iter()))?;
    while let Some(r) = rows.next()? {
        let mut e = row_to_entry(r)?;
        e.attachments = load_attachments(conn, &e.id)?;
        f(&e)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts_query_quotes_terms() {
        assert_eq!(fts_query("  bridge  CR-12 "), Some("\"bridge\"* \"CR-12\"*".into()));
        assert_eq!(fts_query("\"\""), None);
        assert_eq!(fts_query("say \"hi"), Some("\"say\"* \"hi\"*".into()));
    }

    #[test]
    fn like_escape_escapes_wildcards() {
        assert_eq!(like_escape("50%_x"), "%50\\%\\_x%");
    }
}
