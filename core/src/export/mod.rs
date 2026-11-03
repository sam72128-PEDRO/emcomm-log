//! Report exports. All formats share scope resolution and one stable column map so counts,
//! ordering and void handling match across DOCX, XLSX, PDF, CSV and TXT.

pub mod adif;
pub mod csv_out;
pub mod docx;
pub mod forms;
pub mod ics;
pub mod pdf;
pub mod txt;
pub mod xlsx;

use crate::entries::{count_entries, for_each_entry};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::fsutil::{self, short_uuid};
use crate::models::*;
use crate::validate::{format_mhz, parse_utc};
use chrono::{DateTime, FixedOffset, Local, NaiveDate, Offset, TimeZone, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Docx,
    Xlsx,
    Pdf,
    Csv,
    Txt,
    Adif,
}

impl ExportFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            ExportFormat::Docx => "docx",
            ExportFormat::Xlsx => "xlsx",
            ExportFormat::Pdf => "pdf",
            ExportFormat::Csv => "csv",
            ExportFormat::Txt => "txt",
            ExportFormat::Adif => "adi",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportRequest {
    pub format: ExportFormat,
    /// "all", "range", or "filter"
    pub scope: String,
    /// Inclusive local dates (YYYY-MM-DD) interpreted in `display_zone`.
    pub range_from: Option<String>,
    pub range_to: Option<String>,
    pub filter: Option<EntryQuery>,
    pub include_voided: bool,
    /// "local" or "utc"
    pub display_zone: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportPreview {
    pub count: i64,
    pub description: String,
    pub adif_eligible: Option<i64>,
    pub adif_skipped: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedRecord {
    pub sequence_no: i64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportResult {
    pub path: String,
    pub format: ExportFormat,
    pub count: i64,
    pub exported: i64,
    pub skipped: Vec<SkippedRecord>,
    pub warnings: Vec<String>,
    pub bytes: u64,
}

/// Everything a writer needs besides the rows.
pub struct ReportContext {
    pub logbook: LogbookInfo,
    pub generated: String,
    pub zone: DisplayZone,
    pub description: String,
    pub count: i64,
}

#[derive(Debug, Clone, Copy)]
pub enum DisplayZone {
    Utc,
    Local,
}

impl DisplayZone {
    pub fn parse(s: &str) -> DisplayZone {
        if s == "utc" { DisplayZone::Utc } else { DisplayZone::Local }
    }

    pub fn offset_at(&self, t: DateTime<Utc>) -> FixedOffset {
        match self {
            DisplayZone::Utc => FixedOffset::east_opt(0).unwrap(),
            DisplayZone::Local => Local.offset_from_utc_datetime(&t.naive_utc()).fix(),
        }
    }

    /// e.g. "2026-11-02 14:37:05 -05:00" or "2026-11-02 19:37:05Z"
    pub fn format(&self, iso: &str) -> String {
        let Some(t) = parse_utc(iso) else { return iso.to_string() };
        match self {
            DisplayZone::Utc => t.format("%Y-%m-%d %H:%M:%SZ").to_string(),
            DisplayZone::Local => {
                let off = self.offset_at(t);
                t.with_timezone(&off).format("%Y-%m-%d %H:%M:%S %:z").to_string()
            }
        }
    }

    pub fn label(&self) -> String {
        match self {
            DisplayZone::Utc => "UTC".into(),
            DisplayZone::Local => {
                let off = self.offset_at(Utc::now());
                format!("local time (currently UTC{})", off)
            }
        }
    }
}

pub fn fmt_opt(zone: DisplayZone, v: &Option<String>) -> String {
    v.as_deref().map(|s| zone.format(s)).unwrap_or_default()
}

fn day_start_utc(date: &str, zone: DisplayZone) -> AppResult<DateTime<Utc>> {
    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|_| AppError::validation("range", format!("'{date}' is not a date (YYYY-MM-DD)")))?;
    let naive = d.and_hms_opt(0, 0, 0).unwrap();
    Ok(match zone {
        DisplayZone::Utc => Utc.from_utc_datetime(&naive),
        DisplayZone::Local => Local
            .from_local_datetime(&naive)
            .earliest()
            .ok_or_else(|| AppError::validation("range", "Local midnight does not exist on that date"))?
            .with_timezone(&Utc),
    })
}

/// Resolve the export scope into the exact query used for both preview counts and output.
pub fn resolve_query(req: &ExportRequest) -> AppResult<(EntryQuery, String)> {
    let zone = DisplayZone::parse(&req.display_zone);
    let void_note = if req.include_voided { "voided entries included" } else { "voided entries excluded" };
    let mut q = match req.scope.as_str() {
        "all" => {
            let q = EntryQuery { status: if req.include_voided { "all".into() } else { "active".into() }, ..Default::default() };
            (q, format!("Entire logbook; {void_note}"))
        }
        "range" => {
            let from = req.range_from.as_deref().ok_or_else(|| AppError::validation("range_from", "Choose a start date"))?;
            let to = req.range_to.as_deref().ok_or_else(|| AppError::validation("range_to", "Choose an end date"))?;
            let start = day_start_utc(from, zone)?;
            let end_date = NaiveDate::parse_from_str(to, "%Y-%m-%d").map_err(|_| AppError::validation("range_to", "Invalid end date"))?;
            let next = end_date.succ_opt().ok_or_else(|| AppError::validation("range_to", "Invalid end date"))?;
            let end = day_start_utc(&next.format("%Y-%m-%d").to_string(), zone)?;
            if end <= start {
                return Err(AppError::validation("range_to", "End date is before start date"));
            }
            let fmt = |t: DateTime<Utc>| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            let q = EntryQuery {
                status: if req.include_voided { "all".into() } else { "active".into() },
                from_utc: Some(fmt(start)),
                to_utc: Some(fmt(end)),
                ..Default::default()
            };
            let desc = format!(
                "Event dates {from} through {to} inclusive ({}; UTC {} to {} exclusive); {void_note}",
                zone.label(),
                fmt(start),
                fmt(end)
            );
            (q, desc)
        }
        "filter" => {
            let mut q = req.filter.clone().ok_or_else(|| AppError::validation("filter", "No current filter was provided"))?;
            q.status = if req.include_voided { if q.status == "void" { "void".into() } else { "all".into() } } else { "active".into() };
            let desc = format!("Current Traffic Log filter: {}; {void_note}", describe_filter(&q));
            (q, desc)
        }
        other => return Err(AppError::validation("scope", format!("Unknown scope '{other}'"))),
    };
    q.0.sort = "event".into();
    q.0.descending = false;
    q.0.limit = 0;
    q.0.offset = 0;
    Ok(q)
}

pub fn describe_filter(q: &EntryQuery) -> String {
    let mut parts = Vec::new();
    if !q.text.trim().is_empty() {
        parts.push(format!("text \"{}\"", q.text.trim()));
    }
    if let Some(f) = &q.from_utc {
        parts.push(format!("from {f}"));
    }
    if let Some(t) = &q.to_utc {
        parts.push(format!("before {t}"));
    }
    let join = |v: Vec<&str>| v.join("/");
    if !q.kinds.is_empty() {
        parts.push(format!("type {}", join(q.kinds.iter().map(|k| k.as_str()).collect())));
    }
    if !q.directions.is_empty() {
        parts.push(format!("direction {}", join(q.directions.iter().map(|k| k.as_str()).collect())));
    }
    if !q.priorities.is_empty() {
        parts.push(format!("priority {}", join(q.priorities.iter().map(|k| k.as_str()).collect())));
    }
    if !q.handling.is_empty() {
        parts.push(format!("handling {}", join(q.handling.iter().map(|k| k.as_str()).collect())));
    }
    if !q.station.trim().is_empty() {
        parts.push(format!("station \"{}\"", q.station.trim()));
    }
    if !q.tags.is_empty() {
        parts.push(format!("tags {}", q.tags.join(", ")));
    }
    if q.favorites_only {
        parts.push("favorites only".into());
    }
    if q.flagged_only {
        parts.push("flagged only".into());
    }
    if !q.ids.is_empty() {
        parts.push(format!("{} selected entries", q.ids.len()));
    }
    if parts.is_empty() { "no criteria".into() } else { parts.join("; ") }
}

pub fn preview(conn: &Connection, book: LogbookType, req: &ExportRequest) -> AppResult<ExportPreview> {
    let (q, description) = resolve_query(req)?;
    let count = count_entries(conn, &q)?;
    let (mut eligible, mut skipped) = (None, None);
    if req.format == ExportFormat::Adif {
        if book != LogbookType::Ham {
            return Err(AppError::validation("format", "ADIF export is only available for HAM logbooks"));
        }
        let (mut e, mut s) = (0i64, 0i64);
        for_each_entry(conn, &q, |entry| {
            if adif::eligibility(entry).is_ok() {
                e += 1
            } else {
                s += 1
            }
            Ok(())
        })?;
        eligible = Some(e);
        skipped = Some(s);
    }
    Ok(ExportPreview { count, description, adif_eligible: eligible, adif_skipped: skipped })
}

// ---------- column map ----------

pub type Col = (&'static str, fn(&Entry, DisplayZone) -> String);

fn attachment_names(e: &Entry) -> String {
    e.attachments.iter().map(|a| a.display_name.as_str()).collect::<Vec<_>>().join("; ")
}

/// Stable export column order. Documented in docs/EXPORT_FORMATS.md; append only.
pub fn columns(book: LogbookType) -> Vec<Col> {
    let mut cols: Vec<Col> = vec![
        ("Seq", |e, _| e.sequence_no.to_string()),
        ("Status", |e, _| e.status.as_str().to_string()),
        ("Event Time", |e, z| z.format(&e.event_at_utc)),
        ("Event Time UTC", |e, _| e.event_at_utc.clone()),
        ("Entered Zone", |e, _| format!("{} ({})", e.event_timezone, fmt_offset(e.event_utc_offset_min))),
        ("Recorded UTC", |e, _| e.recorded_at_utc.clone()),
        ("Type", |e, _| e.kind.as_str().to_string()),
        ("Direction", |e, _| e.direction.as_str().to_string()),
        ("Priority", |e, _| e.priority.as_str().to_string()),
        ("Handling", |e, _| e.handling.map(|h| h.as_str().to_string()).unwrap_or_default()),
        ("Flagged", |e, _| if e.flagged { "yes".into() } else { String::new() }),
        ("From", |e, _| e.from_text.clone()),
        ("To", |e, _| e.to_text.clone()),
        ("Summary", |e, _| e.subject.clone()),
        ("Message", |e, _| e.body.clone()),
        ("Channel", |e, _| e.channel.clone()),
        ("Location", |e, _| e.location.clone()),
        ("Tags", |e, _| e.tags.join(", ")),
        ("Operator", |e, _| e.operator_snapshot.name.clone()),
        ("Operator Callsign", |e, _| e.operator_snapshot.callsign.clone()),
        ("Revision", |e, _| e.current_revision.to_string()),
        ("Voided UTC", |e, _| e.voided_at_utc.clone().unwrap_or_default()),
        ("Void Reason", |e, _| e.void_reason.clone().unwrap_or_default()),
        ("Attachments", |e, _| attachment_names(e)),
    ];
    match book {
        LogbookType::Ham => cols.extend::<Vec<Col>>(vec![
            ("Call", |e, _| e.ham.as_ref().map(|h| h.their_call.clone()).unwrap_or_default()),
            ("Station Call", |e, _| e.ham.as_ref().map(|h| h.station_call.clone()).unwrap_or_default()),
            ("Operator Call", |e, _| e.ham.as_ref().map(|h| h.operator_call.clone()).unwrap_or_default()),
            ("QSO Start", |e, z| e.ham.as_ref().map(|h| fmt_opt(z, &h.qso_start_utc)).unwrap_or_default()),
            ("QSO End", |e, z| e.ham.as_ref().map(|h| fmt_opt(z, &h.qso_end_utc)).unwrap_or_default()),
            ("Frequency MHz", |e, _| e.ham.as_ref().and_then(|h| h.frequency_hz).map(format_mhz).unwrap_or_default()),
            ("Band", |e, _| e.ham.as_ref().map(|h| h.band.clone()).unwrap_or_default()),
            ("Mode", |e, _| e.ham.as_ref().map(|h| h.mode.clone()).unwrap_or_default()),
            ("Submode", |e, _| e.ham.as_ref().map(|h| h.submode.clone()).unwrap_or_default()),
            ("RST Sent", |e, _| e.ham.as_ref().map(|h| h.rst_sent.clone()).unwrap_or_default()),
            ("RST Rcvd", |e, _| e.ham.as_ref().map(|h| h.rst_rcvd.clone()).unwrap_or_default()),
            ("My Grid", |e, _| e.ham.as_ref().map(|h| h.my_grid.clone()).unwrap_or_default()),
            ("Their Grid", |e, _| e.ham.as_ref().map(|h| h.their_grid.clone()).unwrap_or_default()),
            ("QSO Notes", |e, _| e.ham.as_ref().map(|h| h.qso_notes.clone()).unwrap_or_default()),
        ]),
        LogbookType::Emergency => cols.extend::<Vec<Col>>(vec![
            ("Record Type", |e, _| e.emergency.as_ref().map(|m| m.record_type.as_str().to_string()).unwrap_or_default()),
            ("Incident", |e, _| e.emergency.as_ref().map(|m| m.incident_name.clone()).unwrap_or_default()),
            ("Incident Number", |e, _| e.emergency.as_ref().map(|m| m.incident_number.clone()).unwrap_or_default()),
            ("Operational Period", |e, _| e.emergency.as_ref().map(|m| m.operational_period.clone()).unwrap_or_default()),
            ("Message Number", |e, _| e.emergency.as_ref().map(|m| m.message_number.clone()).unwrap_or_default()),
            ("Precedence", |e, _| e.emergency.as_ref().map(|m| m.precedence.clone()).unwrap_or_default()),
            ("Originator", |e, _| e.emergency.as_ref().map(|m| m.originator.clone()).unwrap_or_default()),
            ("Originator Position", |e, _| e.emergency.as_ref().map(|m| m.originator_position.clone()).unwrap_or_default()),
            ("Recipient", |e, _| e.emergency.as_ref().map(|m| m.recipient.clone()).unwrap_or_default()),
            ("Recipient Position", |e, _| e.emergency.as_ref().map(|m| m.recipient_position.clone()).unwrap_or_default()),
            ("Sender", |e, _| e.emergency.as_ref().map(|m| m.sender.clone()).unwrap_or_default()),
            ("Receiver", |e, _| e.emergency.as_ref().map(|m| m.receiver.clone()).unwrap_or_default()),
            ("Sent Time", |e, z| e.emergency.as_ref().map(|m| fmt_opt(z, &m.sent_at_utc)).unwrap_or_default()),
            ("Received Time", |e, z| e.emergency.as_ref().map(|m| fmt_opt(z, &m.received_at_utc)).unwrap_or_default()),
            ("Reply Requested", |e, _| {
                e.emergency.as_ref().map(|m| if m.reply_requested { "yes".into() } else { String::new() }).unwrap_or_default()
            }),
            ("Action Requested", |e, _| e.emergency.as_ref().map(|m| m.action_requested.clone()).unwrap_or_default()),
            ("Response", |e, _| e.emergency.as_ref().map(|m| m.response.clone()).unwrap_or_default()),
            ("Resource / Unit", |e, _| e.emergency.as_ref().map(|m| m.resource_unit.clone()).unwrap_or_default()),
            ("Assignment", |e, _| e.emergency.as_ref().map(|m| m.assignment.clone()).unwrap_or_default()),
            ("Activity Start", |e, z| e.emergency.as_ref().map(|m| fmt_opt(z, &m.activity_start_utc)).unwrap_or_default()),
            ("Activity End", |e, z| e.emergency.as_ref().map(|m| fmt_opt(z, &m.activity_end_utc)).unwrap_or_default()),
            ("Emergency Location", |e, _| e.emergency.as_ref().map(|m| m.location.clone()).unwrap_or_default()),
            ("Disposition", |e, _| e.emergency.as_ref().map(|m| m.disposition.clone()).unwrap_or_default()),
            ("Preparer", |e, _| e.emergency.as_ref().map(|m| m.preparer.clone()).unwrap_or_default()),
        ]),
        LogbookType::General => {}
    }
    cols
}

pub fn fmt_offset(min: i32) -> String {
    let sign = if min < 0 { '-' } else { '+' };
    let a = min.abs();
    format!("UTC{sign}{:02}:{:02}", a / 60, a % 60)
}

/// Labeled, non-empty detail lines for an entry, used by TXT/DOCX/PDF.
pub fn detail_lines(e: &Entry, book: LogbookType, z: DisplayZone) -> Vec<(String, String)> {
    let skip = ["Seq", "Status", "Event Time", "Event Time UTC", "Type", "Direction", "Priority", "Summary", "Message"];
    columns(book)
        .into_iter()
        .filter(|(h, _)| !skip.contains(h))
        .map(|(h, f)| (h.to_string(), f(e, z)))
        .filter(|(_, v)| !v.is_empty())
        .collect()
}

pub fn heading_line(e: &Entry, z: DisplayZone) -> String {
    let mut s = format!(
        "#{:04}  {}  {} / {} / {}",
        e.sequence_no,
        z.format(&e.event_at_utc),
        e.kind.as_str().to_uppercase(),
        e.direction.as_str(),
        e.priority.as_str()
    );
    if e.status == EntryStatus::Void {
        s.push_str("  [VOIDED]");
    }
    s
}

/// Neutralize spreadsheet formula triggers (OWASP CSV injection guidance).
pub fn is_formula_like(s: &str) -> bool {
    matches!(s.chars().next(), Some('=' | '+' | '-' | '@' | '\t' | '\r'))
}

// ---------- driver ----------

pub fn run_export(conn: &Connection, info: &LogbookInfo, req: &ExportRequest, dest: &Path) -> AppResult<ExportResult> {
    if req.format == ExportFormat::Adif && info.logbook_type != LogbookType::Ham {
        return Err(AppError::validation("format", "ADIF export is only available for HAM logbooks"));
    }
    let (q, description) = resolve_query(req)?;
    let count = count_entries(conn, &q)?;
    let zone = DisplayZone::parse(&req.display_zone);
    let ctx = ReportContext { logbook: info.clone(), generated: zone.format(&fsutil::now_utc()), zone, description, count };
    let parent = dest.parent().ok_or_else(|| AppError::validation("path", "Choose a file location"))?;
    if !parent.is_dir() {
        return Err(AppError::new(ErrorCode::StorageUnavailable, "Export folder is not available"));
    }
    let tmp: PathBuf =
        parent.join(format!(".{}.tmp-{}", dest.file_name().and_then(|n| n.to_str()).unwrap_or("export"), short_uuid(&fsutil::new_uuid())));
    let res = (|| -> AppResult<(i64, Vec<SkippedRecord>, Vec<String>)> {
        let out = match req.format {
            ExportFormat::Csv => csv_out::write(conn, &q, &ctx, &tmp).map(|n| (n, vec![], vec![])),
            ExportFormat::Txt => txt::write(conn, &q, &ctx, &tmp).map(|n| (n, vec![], vec![])),
            ExportFormat::Xlsx => xlsx::write(conn, &q, &ctx, &tmp).map(|(n, w)| (n, vec![], w)),
            ExportFormat::Docx => docx::write(conn, &q, &ctx, &tmp).map(|n| (n, vec![], vec![])),
            ExportFormat::Pdf => pdf::write(conn, &q, &ctx, &tmp).map(|n| (n, vec![], vec![])),
            ExportFormat::Adif => adif::write(conn, &q, &ctx, &tmp),
        }?;
        crate::faults::check("export.before_publish")?;
        fsutil::sync_file(&tmp)?;
        fs::rename(&tmp, dest)?;
        fsutil::sync_dir(parent)?;
        Ok(out)
    })();
    match res {
        Ok((exported, skipped, warnings)) => Ok(ExportResult {
            path: dest.to_string_lossy().to_string(),
            format: req.format,
            count,
            exported,
            skipped,
            warnings,
            bytes: fs::metadata(dest).map(|m| m.len()).unwrap_or(0),
        }),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}
