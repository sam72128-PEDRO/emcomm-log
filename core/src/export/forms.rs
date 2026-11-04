//! ICS 213 and ICS 214 exports from logbook data (V2 spec §4.2, §4.3), written onto FEMA's forms via
//! [`FormBuilder`]. Voided entries never appear; signature lines stay blank; text that does not fit
//! a block continues on a continuation page that names the block.

use super::ics::{FormBuilder, ICS_213, ICS_214};
use super::pdf::{FontMetrics, wrap};
use super::review::{self, ExportReview, ReviewRef, check_unchanged};
use super::{DisplayZone, SkippedRecord};
use crate::entries::load_entry;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::fsutil::{self, now_utc, short_uuid};
use crate::logbook::APP_VERSION;
use crate::models::{Entry, EntryKind, EntryStatus, LogbookInfo, LogbookType, OpResource};
use crate::periods::{get_period, list_resources};
use crate::validate::parse_utc;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params_from_iter};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Activity rows per 214 page: 24 on the first page, 36 on each continuation page.
const ROWS_FIRST: usize = 24;
const ROWS_CONT: usize = 36;
const RESOURCE_ROWS: usize = 8;
/// "Notable Activities" cell width minus padding, and the size rows are wrapped at.
const ACTIVITY_WIDTH: f32 = 440.0;
const ROW_SIZE: f32 = 9.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Form213Request {
    pub entry_ids: Vec<String>,
    /// "local" or "utc"
    pub display_zone: String,
    /// 2.2: the messages (and revisions) approved in Review; None = every eligible selected message.
    #[serde(default)]
    pub reviewed: Option<Vec<ReviewRef>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Form214Request {
    pub op_id: String,
    pub name: String,
    pub ics_position: String,
    pub home_agency: String,
    pub kinds: Vec<EntryKind>,
    pub include_details: bool,
    pub prepared_by_name: String,
    pub prepared_by_position: String,
    pub display_zone: String,
    /// 2.2: restrict to these entries (a Traffic Log selection); empty = every eligible entry of the period.
    pub entry_ids: Vec<String>,
    /// 2.2: the entries (and revisions) approved in Review.
    pub reviewed: Option<Vec<ReviewRef>>,
}

impl Default for Form214Request {
    fn default() -> Self {
        Form214Request {
            op_id: String::new(),
            name: String::new(),
            ics_position: String::new(),
            home_agency: String::new(),
            kinds: vec![EntryKind::Activity],
            include_details: false,
            prepared_by_name: String::new(),
            prepared_by_position: String::new(),
            display_zone: "local".into(),
            entry_ids: vec![],
            reviewed: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormPreview {
    pub count: i64,
    pub skipped: Vec<SkippedRecord>,
}

/// A finished form in memory: the PDF plus what went into it.
pub struct BuiltForm {
    pub pdf: Vec<u8>,
    pub exported: i64,
    pub pages: i64,
    pub skipped: Vec<SkippedRecord>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormResult {
    pub path: String,
    pub form: String,
    pub exported: i64,
    pub pages: i64,
    pub skipped: Vec<SkippedRecord>,
    pub warnings: Vec<String>,
    pub bytes: u64,
}

pub(crate) fn local_parts(zone: DisplayZone, iso: &str) -> Option<(String, String)> {
    let t: DateTime<Utc> = parse_utc(iso)?;
    Some(match zone {
        DisplayZone::Utc => (t.format("%Y-%m-%d").to_string(), t.format("%H:%MZ").to_string()),
        DisplayZone::Local => {
            let l = t.with_timezone(&zone.offset_at(t));
            (l.format("%Y-%m-%d").to_string(), l.format("%H:%M").to_string())
        }
    })
}

/// "2026-11-03 08:08 -05:00" or "2026-11-03 13:08Z"
pub(crate) fn date_time(zone: DisplayZone, iso: &str) -> String {
    let Some(t) = parse_utc(iso) else { return iso.to_string() };
    match zone {
        DisplayZone::Utc => t.format("%Y-%m-%d %H:%MZ").to_string(),
        DisplayZone::Local => t.with_timezone(&zone.offset_at(t)).format("%Y-%m-%d %H:%M %:z").to_string(),
    }
}

fn join_nonempty(parts: &[&str]) -> String {
    parts.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ")
}

pub(crate) fn footer_base(info: &LogbookInfo) -> String {
    format!("Prepared with EMCOMM LOG {APP_VERSION} from logbook \u{201c}{}\u{201d}", info.name)
}

pub(crate) fn zone_note(zone: DisplayZone) -> String {
    match zone {
        DisplayZone::Utc => "times in UTC".into(),
        DisplayZone::Local => format!("times in local time (UTC{})", zone.offset_at(Utc::now())),
    }
}

pub(crate) fn require_emergency(info: &LogbookInfo) -> AppResult<()> {
    if info.logbook_type != LogbookType::Emergency {
        return Err(AppError::validation("form", "ICS forms are available in Emergency logbooks"));
    }
    Ok(())
}

/// Each requested entry and whether it can go on an ICS 213 (and why not).
fn candidates_213(conn: &Connection, ids: &[String]) -> AppResult<Vec<(Entry, Result<(), String>)>> {
    let mut out: Vec<(Entry, Result<(), String>)> = Vec::new();
    for id in ids {
        let e = load_entry(conn, id)?;
        let ok = if e.status == EntryStatus::Void {
            Err("voided".to_string())
        } else if e.kind != EntryKind::Message || e.emergency.is_none() {
            Err("not a Message entry (the ICS 213 is for messages)".to_string())
        } else {
            Ok(())
        };
        out.push((e, ok));
    }
    out.sort_by_key(|(e, _)| e.sequence_no);
    Ok(out)
}

/// Split candidates into what goes on the form and what is skipped, honouring a Review (2.2 EXP-04/08):
/// only reviewed entries, refused if any changed or cannot legally go on this form.
pub(crate) fn plan_reviewed(
    conn: &Connection,
    cands: Vec<(Entry, Result<(), String>)>,
    reviewed: &Option<Vec<ReviewRef>>,
) -> AppResult<(Vec<Entry>, Vec<SkippedRecord>)> {
    if let Some(r) = reviewed {
        check_unchanged(conn, r)?;
        for x in r {
            match cands.iter().find(|(e, _)| e.id == x.id) {
                Some((_, Ok(()))) => {}
                Some((e, Err(why))) => {
                    return Err(AppError::validation("reviewed", format!("Entry #{} cannot go on this form: {why}", e.sequence_no)));
                }
                None => return Err(AppError::validation("reviewed", "A reviewed entry is not part of this form's scope")),
            }
        }
    }
    let mut ok = Vec::new();
    let mut skipped = Vec::new();
    for (e, elig) in cands {
        match elig {
            Err(why) => skipped.push(SkippedRecord { sequence_no: e.sequence_no, reason: why }),
            Ok(()) => match reviewed {
                Some(r) if !r.iter().any(|x| x.id == e.id) => {
                    skipped.push(SkippedRecord { sequence_no: e.sequence_no, reason: "left out in review".into() })
                }
                _ => ok.push(e),
            },
        }
    }
    skipped.sort_by_key(|s| s.sequence_no);
    Ok((ok, skipped))
}

fn plan_213(conn: &Connection, req: &Form213Request) -> AppResult<(Vec<Entry>, Vec<SkippedRecord>)> {
    let cands = candidates_213(conn, &req.entry_ids)?;
    plan_reviewed(conn, cands, &req.reviewed)
}

/// 2.2: one message per ICS 213; non-messages and voided entries listed with the reason.
pub fn review_213(conn: &Connection, info: &LogbookInfo, req: &Form213Request) -> AppResult<ExportReview> {
    require_emergency(info)?;
    let rows = candidates_213(conn, &req.entry_ids)?.into_iter().map(|(e, ok)| review::row(&e, ok)).collect();
    Ok(review::build(rows, vec!["Each message becomes its own ICS 213.".into()]))
}

pub fn preview_213(conn: &Connection, info: &LogbookInfo, req: &Form213Request) -> AppResult<FormPreview> {
    require_emergency(info)?;
    let (ok, skipped) = plan_213(conn, req)?;
    Ok(FormPreview { count: ok.len() as i64, skipped })
}

pub fn build_213(conn: &Connection, info: &LogbookInfo, req: &Form213Request) -> AppResult<BuiltForm> {
    require_emergency(info)?;
    let zone = DisplayZone::parse(&req.display_zone);
    let (entries, skipped) = plan_213(conn, req)?;
    if entries.is_empty() {
        return Err(AppError::validation("entries", "None of the selected entries can go on an ICS 213 (only active Message entries can)"));
    }
    let mut warnings = Vec::new();
    let mut b = FormBuilder::new(ICS_213)?;
    for (i, e) in entries.iter().enumerate() {
        let m = e.emergency.as_ref().expect("planned entries are messages");
        let sfx = if i == 0 { String::new() } else { format!(" ({})", i + 1) };
        let mut footer = format!("{} \u{00b7} entry #{}", footer_base(info), e.sequence_no);
        if !m.message_number.is_empty() {
            footer.push_str(&format!(" \u{00b7} message no. {}", m.message_number));
        }
        b.add_page(1, &sfx, &footer)?;
        let f = |n: &str| format!("{n}{sfx}");
        let incident = if m.incident_name.is_empty() { info.incident.incident_name.clone() } else { m.incident_name.clone() };
        let to = match join_nonempty(&[&m.recipient, &m.recipient_position]) {
            s if s.is_empty() => e.to_text.clone(),
            s => s,
        };
        let from = match join_nonempty(&[&m.originator, &m.originator_position]) {
            s if s.is_empty() => e.from_text.clone(),
            s => s,
        };
        if to.is_empty() {
            warnings.push(format!("#{}: no recipient, so block 2 (To) is blank", e.sequence_no));
        }
        let when = m.sent_at_utc.clone().unwrap_or_else(|| e.event_at_utc.clone());
        let (date, time) = local_parts(zone, &when).unwrap_or_default();
        let mut over: Vec<(&str, String)> = Vec::new();
        let mut put = |b: &mut FormBuilder, field: &str, label: &'static str, v: &str| -> AppResult<()> {
            if let Some(rest) = b.set(&f(field), v)? {
                over.push((label, rest));
            }
            Ok(())
        };
        put(&mut b, "1 Incident Name Optional", "1. Incident Name", &incident)?;
        put(&mut b, "2 To Name and Position", "2. To", &to)?;
        put(&mut b, "3 From Name and Position", "3. From", &from)?;
        put(&mut b, "4 Subject", "4. Subject", &e.subject)?;
        put(&mut b, "5 Date", "5. Date", &date)?;
        put(
            &mut b,
            "6 Time",
            "6. Time",
            &format!("{time}{}", if matches!(zone, DisplayZone::Local) { local_offset(zone, &when) } else { String::new() }),
        )?;
        put(&mut b, "7 Message", "7. Message", &e.body)?;
        put(&mut b, "8 Approved by Name", "8. Approved by (name)", &m.approved_by_name)?;
        put(&mut b, "PositionTitle_13", "8. Approved by (position/title)", &m.approved_by_position)?;
        put(&mut b, "9 Reply", "9. Reply", &m.response)?;
        put(&mut b, "10 Replied by Name", "10. Replied by (name)", &m.replied_by_name)?;
        put(&mut b, "PositionTitle_14", "10. Replied by (position/title)", &m.replied_by_position)?;
        if let Some(t) = &m.replied_at_utc {
            put(&mut b, "DateTime_14", "10. Replied by (date/time)", &date_time(zone, t))?;
        }
        if !over.is_empty() {
            let body = over.iter().map(|(label, rest)| format!("{label} (continued):\n{rest}")).collect::<Vec<_>>().join("\n\n");
            b.add_text_pages(&format!("ICS 213 continuation \u{2014} entry #{}: {}", e.sequence_no, e.subject), &body, &footer)?;
            warnings.push(format!("#{}: text longer than its block continues on a continuation page", e.sequence_no));
        }
    }
    let pages = b.page_count() as i64;
    let pdf = b.finish(&format!("ICS 213 General Message \u{2014} {}", info.name))?;
    Ok(BuiltForm { pdf, exported: entries.len() as i64, pages, skipped, warnings })
}

fn local_offset(zone: DisplayZone, iso: &str) -> String {
    parse_utc(iso).map(|t| format!(" {}", zone.offset_at(t))).unwrap_or_default()
}

/// Entries for a 214: active, of the chosen kinds, and either assigned to this period or unassigned
/// with an event time inside it. Ordered by event time, then sequence.
pub(crate) fn entries_for_period(conn: &Connection, op_id: &str, start: &str, end: &str, kinds: &[EntryKind]) -> AppResult<Vec<Entry>> {
    if kinds.is_empty() {
        return Err(AppError::validation("kinds", "Choose at least one entry type to include"));
    }
    let marks = vec!["?"; kinds.len()].join(",");
    let sql = format!(
        "SELECT id FROM entries WHERE status='active' AND kind IN ({marks})
           AND (op_id = ? OR (op_id IS NULL AND event_at_utc >= ? AND event_at_utc < ?))
         ORDER BY event_at_utc, sequence_no"
    );
    let mut vals: Vec<String> = kinds.iter().map(|k| k.as_str().to_string()).collect();
    vals.extend([op_id.to_string(), start.to_string(), end.to_string()]);
    let mut stmt = conn.prepare(&sql)?;
    let ids = stmt.query_map(params_from_iter(vals.iter()), |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    ids.iter().map(|id| load_entry(conn, id)).collect()
}

/// The entries an ICS 214 for this period can list: the period's entries of the chosen types, or the
/// selected entries with a reason for each one outside the period (never silently added, EXP-05).
fn candidates_214(conn: &Connection, req: &Form214Request) -> AppResult<Vec<(Entry, Result<(), String>)>> {
    let op = get_period(conn, &req.op_id)?;
    if op.status == EntryStatus::Void {
        return Err(AppError::validation("op_id", "That operational period is voided"));
    }
    let all = entries_for_period(conn, &op.id, &op.start_utc, &op.end_utc, &req.kinds)?;
    if req.entry_ids.is_empty() {
        return Ok(all.into_iter().map(|e| (e, Ok(()))).collect());
    }
    let mut out = Vec::new();
    for id in &req.entry_ids {
        match all.iter().find(|e| &e.id == id) {
            Some(e) => out.push((e.clone(), Ok(()))),
            None => {
                let e = load_entry(conn, id)?;
                let why = if e.status == EntryStatus::Void {
                    "voided".to_string()
                } else if !req.kinds.contains(&e.kind) {
                    format!("{} entries are not included (choose the entry types to list)", e.kind.as_str())
                } else {
                    format!("outside operational period \u{201c}{}\u{201d}", op.name)
                };
                out.push((e, Err(why)));
            }
        }
    }
    out.sort_by(|(a, _), (b, _)| (&a.event_at_utc, a.sequence_no).cmp(&(&b.event_at_utc, b.sequence_no)));
    Ok(out)
}

pub fn review_214(conn: &Connection, info: &LogbookInfo, req: &Form214Request) -> AppResult<ExportReview> {
    require_emergency(info)?;
    let op = get_period(conn, &req.op_id)?;
    let rows = candidates_214(conn, req)?.into_iter().map(|(e, ok)| review::row(&e, ok)).collect();
    Ok(review::build(
        rows,
        vec![format!("Operational period \u{201c}{}\u{201d}: entries assigned to it, plus unassigned entries timed inside it.", op.name)],
    ))
}

pub fn preview_214(conn: &Connection, info: &LogbookInfo, req: &Form214Request) -> AppResult<FormPreview> {
    require_emergency(info)?;
    let (ok, skipped) = plan_reviewed(conn, candidates_214(conn, req)?, &req.reviewed)?;
    Ok(FormPreview { count: ok.len() as i64, skipped })
}

fn activity_text(e: &Entry, zone: DisplayZone, details: bool) -> String {
    let mut t = String::new();
    if e.kind != EntryKind::Activity {
        let kind = e.kind.as_str();
        t.push_str(&format!("[{}{}] ", kind[..1].to_uppercase(), &kind[1..]));
    }
    if let Some(m) = &e.emergency
        && let (Some(a), Some(z)) = (&m.activity_start_utc, &m.activity_end_utc)
        && let (Some((_, ta)), Some((_, tz))) = (local_parts(zone, a), local_parts(zone, z))
    {
        t.push_str(&format!("({ta}\u{2013}{tz}) "));
    }
    t.push_str(&e.subject);
    if details && !e.body.trim().is_empty() {
        t.push_str(" \u{2014} ");
        t.push_str(&e.body.split_whitespace().collect::<Vec<_>>().join(" "));
    }
    t
}

/// Field names of activity row `i` (1-based) on the first page or a continuation page.
fn row_fields(first_page: bool, i: usize) -> (String, String) {
    if first_page || i > 24 {
        (format!("DateTimeRow{i}"), format!("Notable ActivitiesRow{i}"))
    } else {
        (format!("DateTimeRow{i}_2"), format!("Notable ActivitiesRow{i}_2"))
    }
}

fn resource_fields(i: usize) -> (String, String, String) {
    let name = if i <= 6 { format!("NameRow{i}_3") } else { format!("NameRow{i}") };
    (name, format!("ICS PositionRow{i}"), format!("Home Agency and UnitRow{i}"))
}

pub fn build_214(conn: &Connection, info: &LogbookInfo, req: &Form214Request) -> AppResult<BuiltForm> {
    require_emergency(info)?;
    let zone = DisplayZone::parse(&req.display_zone);
    let op = get_period(conn, &req.op_id)?;
    if op.status == EntryStatus::Void {
        return Err(AppError::validation("op_id", "That operational period is voided"));
    }
    let (entries, skipped_214) = plan_reviewed(conn, candidates_214(conn, req)?, &req.reviewed)?;
    let resources: Vec<OpResource> = list_resources(conn, &op.id)?;
    let fm = FontMetrics::load()?;
    // Each entry becomes one or more rows; continuation rows leave Date/Time blank.
    let mut rows: Vec<(String, String)> = Vec::new();
    for e in &entries {
        let when = date_time(zone, &e.event_at_utc);
        let when = when.rsplit_once(' ').filter(|_| matches!(zone, DisplayZone::Local)).map(|(a, _)| a.to_string()).unwrap_or(when);
        for (k, line) in wrap(&fm, &activity_text(e, zone, req.include_details), ACTIVITY_WIDTH, ROW_SIZE).into_iter().enumerate() {
            rows.push((if k == 0 { when.clone() } else { String::new() }, line));
        }
    }
    let cont_pages = rows.len().saturating_sub(ROWS_FIRST).div_ceil(ROWS_CONT);
    let footer = format!(
        "{} \u{00b7} operational period \u{201c}{}\u{201d} \u{00b7} {} entries \u{00b7} {}",
        footer_base(info),
        op.name,
        entries.len(),
        zone_note(zone)
    );
    let mut b = FormBuilder::new(ICS_214)?;
    let (df, tf) = local_parts(zone, &op.start_utc).unwrap_or_default();
    let (dt, tt) = local_parts(zone, &op.end_utc).unwrap_or_default();
    let prepared = date_time(zone, &now_utc());
    let mut warnings = Vec::new();
    let mut row_iter = rows.into_iter();
    for page in 0..=cont_pages {
        let first = page == 0;
        let sfx = if first { String::new() } else { format!(" ({})", page + 1) };
        b.add_page(if first { 1 } else { 2 }, &sfx, &footer)?;
        let f = |n: &str| format!("{n}{sfx}");
        b.set(&f(if first { "1 Incident Name_19" } else { "1 Incident Name_20" }), &info.incident.incident_name)?;
        b.set(&f("Date From"), &df)?;
        b.set(&f("Time From"), &tf)?;
        b.set(&f("Date To"), &dt)?;
        b.set(&f("Time To"), &tt)?;
        let (pn, pp, pd) = if first {
            ("8 Prepared by Name", "PositionTitle_15", "DateTime_15")
        } else {
            ("8 Prepared by Name_2", "PositionTitle_16", "DateTime_16")
        };
        b.set(&f(pn), &req.prepared_by_name)?;
        b.set(&f(pp), &req.prepared_by_position)?;
        b.set(&f(pd), &prepared)?;
        if first {
            b.set("3 Name", &req.name)?;
            b.set("4 ICS Position", &req.ics_position)?;
            b.set("5 Home Agency and Unit", &req.home_agency)?;
            for (i, r) in resources.iter().take(RESOURCE_ROWS).enumerate() {
                let (n, p, a) = resource_fields(i + 1);
                b.set(&n, &r.name)?;
                b.set(&p, &r.ics_position)?;
                b.set(&a, &r.home_agency)?;
            }
        }
        let capacity = if first { ROWS_FIRST } else { ROWS_CONT };
        for i in 1..=capacity {
            let Some((when, text)) = row_iter.next() else { break };
            let (fd, fa) = row_fields(first, i);
            b.set(&f(&fd), &when)?;
            if let Some(rest) = b.set(&f(&fa), &text)? {
                return Err(AppError::internal(format!("ICS 214 row did not fit its cell: {rest}")));
            }
        }
    }
    if resources.len() > RESOURCE_ROWS {
        let body = resources
            .iter()
            .enumerate()
            .skip(RESOURCE_ROWS)
            .map(|(i, r)| format!("{}. {} \u{2014} {} \u{2014} {}", i + 1, r.name, r.ics_position, r.home_agency))
            .collect::<Vec<_>>()
            .join("\n");
        b.add_text_pages("ICS 214 continuation \u{2014} 6. Resources Assigned (continued)", &body, &footer)?;
        warnings.push(format!("{} resources did not fit block 6 and continue on a continuation page", resources.len() - RESOURCE_ROWS));
    }
    if entries.is_empty() {
        warnings.push("No entries matched this operational period and these entry types; the activity log is empty.".into());
    }
    let pages = b.page_count() as i64;
    let pdf = b.finish(&format!("ICS 214 Activity Log \u{2014} {} \u{2014} {}", info.name, op.name))?;
    Ok(BuiltForm { pdf, exported: entries.len() as i64, pages, skipped: skipped_214, warnings })
}

/// Write bytes to `dest` atomically (temp file, sync, rename), like the other exports.
pub fn publish(bytes: &[u8], dest: &Path) -> AppResult<u64> {
    let parent = dest.parent().ok_or_else(|| AppError::validation("path", "Choose a file location"))?;
    if !parent.is_dir() {
        return Err(AppError::new(ErrorCode::StorageUnavailable, "Export folder is not available"));
    }
    let tmp: PathBuf =
        parent.join(format!(".{}.tmp-{}", dest.file_name().and_then(|n| n.to_str()).unwrap_or("form"), short_uuid(&fsutil::new_uuid())));
    let res = (|| -> AppResult<()> {
        fs::write(&tmp, bytes)?;
        crate::faults::check("export.before_publish")?;
        fsutil::sync_file(&tmp)?;
        fs::rename(&tmp, dest)?;
        fsutil::sync_dir(parent)?;
        Ok(())
    })();
    if let Err(e) = res {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(bytes.len() as u64)
}

pub fn export_213(conn: &Connection, info: &LogbookInfo, req: &Form213Request, dest: &Path) -> AppResult<FormResult> {
    let f = build_213(conn, info, req)?;
    let bytes = publish(&f.pdf, dest)?;
    Ok(FormResult {
        path: dest.to_string_lossy().to_string(),
        form: "ics213".into(),
        exported: f.exported,
        pages: f.pages,
        skipped: f.skipped,
        warnings: f.warnings,
        bytes,
    })
}

pub fn export_214(conn: &Connection, info: &LogbookInfo, req: &Form214Request, dest: &Path) -> AppResult<FormResult> {
    let f = build_214(conn, info, req)?;
    let bytes = publish(&f.pdf, dest)?;
    Ok(FormResult {
        path: dest.to_string_lossy().to_string(),
        form: "ics214".into(),
        exported: f.exported,
        pages: f.pages,
        skipped: f.skipped,
        warnings: f.warnings,
        bytes,
    })
}
