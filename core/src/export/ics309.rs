//! ICS 309 Communications Log (V2 spec §4.4). FEMA's NIMS form set has no fillable ICS 309, so the
//! form is drawn here in the standard layout: blocks 1–4 on every page, the log table (Time, FROM
//! call sign/ID and message number, TO call sign/ID and message number, Message), and blocks 6–8.
//! Every cell wraps onto extra lines instead of being cut; voided entries never appear.

use super::forms::{
    BuiltForm, FormPreview, FormResult, date_time, entries_for_period, footer_base, local_parts, publish, require_emergency, zone_note,
};
use super::ics::DrawnDoc;
use super::review::{self, ExportReview, ReviewRef};
use super::{DisplayZone, SkippedRecord};
use crate::error::{AppError, AppResult};
use crate::fsutil::now_utc;
use crate::models::{Direction, Entry, EntryKind, EntryStatus, LogbookInfo};
use crate::periods::get_period;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::Path;

const LEFT: f32 = 36.0;
const RIGHT: f32 = 576.0;
const SIZE: f32 = 8.0;
const ROW_H: f32 = 12.5;
/// Column x positions: Time | FROM id | FROM msg # | TO id | TO msg # | Message.
const COLS: [f32; 7] = [36.0, 102.0, 186.0, 220.0, 304.0, 338.0, 576.0];
const HEADER_TOP: f32 = 740.0;
const TABLE_TOP: f32 = 642.0;
const FOOTER_TOP: f32 = 82.0;
pub const ROWS_PER_PAGE: usize = ((TABLE_TOP - FOOTER_TOP) / ROW_H) as usize;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Form309Request {
    pub op_id: String,
    /// Restrict to these entries (a Traffic Log selection); empty = every matching entry in the period.
    pub entry_ids: Vec<String>,
    pub kinds: Vec<EntryKind>,
    pub include_details: bool,
    pub net_name: String,
    pub operator_name: String,
    pub operator_call: String,
    pub prepared_by_name: String,
    pub prepared_by_call: String,
    pub display_zone: String,
    /// 2.2: the entries (and revisions) approved in Review.
    pub reviewed: Option<Vec<ReviewRef>>,
}

impl Default for Form309Request {
    fn default() -> Self {
        Form309Request {
            op_id: String::new(),
            entry_ids: vec![],
            kinds: vec![EntryKind::Traffic, EntryKind::Message],
            include_details: false,
            net_name: String::new(),
            operator_name: String::new(),
            operator_call: String::new(),
            prepared_by_name: String::new(),
            prepared_by_call: String::new(),
            display_zone: "local".into(),
            reviewed: None,
        }
    }
}

/// Entries for the log and the selected entries that are left out (with why).
/// Each candidate entry and whether it can go on this ICS 309 (and why not).
fn candidates(conn: &Connection, req: &Form309Request) -> AppResult<Vec<(Entry, Result<(), String>)>> {
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
                let e = crate::entries::load_entry(conn, id)?;
                let reason = if e.status == EntryStatus::Void {
                    "voided".to_string()
                } else if !req.kinds.contains(&e.kind) {
                    format!("{} entries are not included (choose the entry types to list)", e.kind.as_str())
                } else {
                    format!("not in operational period \u{201c}{}\u{201d}", op.name)
                };
                out.push((e, Err(reason)));
            }
        }
    }
    out.sort_by(|(a, _), (b, _)| (&a.event_at_utc, a.sequence_no).cmp(&(&b.event_at_utc, b.sequence_no)));
    Ok(out)
}

fn plan(conn: &Connection, req: &Form309Request) -> AppResult<(Vec<Entry>, Vec<SkippedRecord>)> {
    super::forms::plan_reviewed(conn, candidates(conn, req)?, &req.reviewed)
}

/// 2.2: the 309 lines that would be written, with FROM/TO and summary, and why others are left out.
pub fn review_309(conn: &Connection, info: &LogbookInfo, req: &Form309Request) -> AppResult<ExportReview> {
    require_emergency(info)?;
    let rows = candidates(conn, req)?.into_iter().map(|(e, ok)| review::row(&e, ok)).collect();
    Ok(review::build(rows, vec!["One log line per entry: time, FROM, TO and the message summary.".into()]))
}

pub fn preview_309(conn: &Connection, info: &LogbookInfo, req: &Form309Request) -> AppResult<FormPreview> {
    require_emergency(info)?;
    let (kept, skipped) = plan(conn, req)?;
    Ok(FormPreview { count: kept.len() as i64, skipped })
}

/// One printed line of the log; an entry takes as many lines as its longest cell needs.
#[derive(Default, Clone)]
struct Line {
    cells: [String; 6],
}

fn message_number_cols(e: &Entry) -> (String, String) {
    let n = e.emergency.as_ref().map(|m| m.message_number.clone()).unwrap_or_default();
    if e.direction == Direction::Sent { (String::new(), n) } else { (n, String::new()) }
}

fn lines_for(doc: &DrawnDoc, e: &Entry, time: String, details: bool) -> Vec<Line> {
    let mut message = e.subject.clone();
    if details && !e.body.trim().is_empty() {
        message.push_str(" \u{2014} ");
        message.push_str(&e.body.split_whitespace().collect::<Vec<_>>().join(" "));
    }
    let (from_no, to_no) = message_number_cols(e);
    let values = [time, e.from_text.clone(), from_no, e.to_text.clone(), to_no, message];
    let wrapped: Vec<Vec<String>> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let w = COLS[i + 1] - COLS[i] - 6.0;
            if v.is_empty() { vec![] } else { doc.wrap(v, w, SIZE) }
        })
        .collect();
    let n = wrapped.iter().map(Vec::len).max().unwrap_or(0).max(1);
    (0..n).map(|k| Line { cells: std::array::from_fn(|i| wrapped[i].get(k).cloned().unwrap_or_default()) }).collect()
}

fn hline(y: f32, x1: f32, x2: f32, w: f32, gray: f32) -> String {
    format!("{w} w {gray} G {x1:.2} {y:.2} m {x2:.2} {y:.2} l S\n")
}

fn vline(x: f32, y1: f32, y2: f32, w: f32) -> String {
    format!("{w} w 0 G {x:.2} {y1:.2} m {x:.2} {y2:.2} l S\n")
}

struct Header {
    incident: String,
    from: String,
    to: String,
    net: String,
    operator: String,
    prepared_by: String,
    prepared_at: String,
    footer: String,
}

fn draw_page(doc: &mut DrawnDoc, h: &Header, lines: &[Line], page: usize, pages: usize) -> String {
    let mut o = String::new();
    let title = "COMMUNICATIONS LOG (ICS 309)";
    o.push_str(&doc.text((LEFT + RIGHT - doc.width(title, 14.0)) / 2.0, HEADER_TOP + 10.0, 14.0, title));
    // Blocks 1–4: two rows of two boxes.
    let mid = 306.0;
    o.push_str(&format!("1 w 0 G {LEFT} {:.2} {:.2} {:.2} re S\n", FOOTER_TOP - 32.0, RIGHT - LEFT, HEADER_TOP - FOOTER_TOP + 32.0));
    for y in [HEADER_TOP - 30.0, HEADER_TOP - 60.0] {
        o.push_str(&hline(y, LEFT, RIGHT, 1.0, 0.0));
    }
    o.push_str(&vline(mid, HEADER_TOP, HEADER_TOP - 60.0, 1.0));
    let label = |doc: &mut DrawnDoc, x: f32, y: f32, s: &str| doc.text(x, y, 7.5, s);
    o.push_str(&label(doc, LEFT + 4.0, HEADER_TOP - 10.0, "1. Incident Name:"));
    o.push_str(&doc.text_fit(LEFT + 4.0, HEADER_TOP - 24.0, 9.0, mid - LEFT - 8.0, &h.incident));
    o.push_str(&label(doc, mid + 4.0, HEADER_TOP - 10.0, "2. Operational Period:"));
    o.push_str(&doc.text(mid + 4.0, HEADER_TOP - 24.0, 8.5, &format!("From: {}", h.from)));
    o.push_str(&doc.text(mid + 140.0, HEADER_TOP - 24.0, 8.5, &format!("To: {}", h.to)));
    o.push_str(&label(doc, LEFT + 4.0, HEADER_TOP - 40.0, "3. Radio Net Name or Position/Tactical Call:"));
    o.push_str(&doc.text_fit(LEFT + 4.0, HEADER_TOP - 54.0, 9.0, mid - LEFT - 8.0, &h.net));
    o.push_str(&label(doc, mid + 4.0, HEADER_TOP - 40.0, "4. Radio Operator (Name, Call Sign):"));
    o.push_str(&doc.text_fit(mid + 4.0, HEADER_TOP - 54.0, 9.0, RIGHT - mid - 8.0, &h.operator));
    // Block 5 heading and column heads.
    let head_top = HEADER_TOP - 60.0;
    o.push_str(&label(doc, LEFT + 4.0, head_top - 10.0, "5. Communications Log:"));
    o.push_str(&hline(head_top - 14.0, LEFT, RIGHT, 0.75, 0.0));
    let (g1, g2) = ((COLS[1] + COLS[3]) / 2.0, (COLS[3] + COLS[5]) / 2.0);
    for (x, s) in [(g1, "FROM"), (g2, "TO")] {
        o.push_str(&doc.text(x - doc.width(s, 7.5) / 2.0, head_top - 24.0, 7.5, s));
    }
    o.push_str(&hline(head_top - 27.0, COLS[1], COLS[5], 0.5, 0.0));
    let subs = ["Time", "Call Sign/ID", "Msg #", "Call Sign/ID", "Msg #", "Message"];
    for (i, s) in subs.iter().enumerate() {
        let cx = (COLS[i] + COLS[i + 1]) / 2.0;
        o.push_str(&doc.text(cx - doc.width(s, 7.5) / 2.0, head_top - 35.0, 7.5, s));
    }
    o.push_str(&hline(TABLE_TOP, LEFT, RIGHT, 0.75, 0.0));
    for (i, x) in COLS.iter().enumerate().skip(1).take(5) {
        // FROM/TO group dividers run through both header lines; inner dividers only below "FROM"/"TO".
        let top = if i == 1 || i == 3 || i == 5 { head_top - 14.0 } else { head_top - 27.0 };
        o.push_str(&vline(*x, top, FOOTER_TOP, 0.5));
    }
    // Rows.
    for r in 0..ROWS_PER_PAGE {
        let y = TABLE_TOP - (r as f32 + 1.0) * ROW_H;
        o.push_str(&hline(y, LEFT, RIGHT, 0.3, 0.6));
        if let Some(line) = lines.get(r) {
            for (i, cell) in line.cells.iter().enumerate() {
                if !cell.is_empty() {
                    o.push_str(&doc.text(COLS[i] + 3.0, y + 3.5, SIZE, cell));
                }
            }
        }
    }
    // Blocks 6–8.
    o.push_str(&hline(FOOTER_TOP, LEFT, RIGHT, 1.0, 0.0));
    let (c1, c2) = (306.0, 470.0);
    o.push_str(&vline(c1, FOOTER_TOP, FOOTER_TOP - 32.0, 1.0));
    o.push_str(&vline(c2, FOOTER_TOP, FOOTER_TOP - 32.0, 1.0));
    o.push_str(&label(doc, LEFT + 4.0, FOOTER_TOP - 10.0, "6. Prepared By (Name, Call Sign):"));
    o.push_str(&doc.text_fit(LEFT + 4.0, FOOTER_TOP - 24.0, 9.0, c1 - LEFT - 8.0, &h.prepared_by));
    o.push_str(&label(doc, c1 + 4.0, FOOTER_TOP - 10.0, "7. Date & Time Prepared:"));
    o.push_str(&doc.text(c1 + 4.0, FOOTER_TOP - 24.0, 9.0, &h.prepared_at));
    o.push_str(&label(doc, c2 + 4.0, FOOTER_TOP - 10.0, "8."));
    o.push_str(&doc.text(c2 + 16.0, FOOTER_TOP - 24.0, 9.0, &format!("Page {page} of {pages}")));
    o.push_str(&doc.text(LEFT, 22.0, 6.5, &format!("{}   Page {page} of {pages}", h.footer)));
    o
}

fn log_lines(doc: &DrawnDoc, entries: &[Entry], zone: DisplayZone, start: &str, end: &str, details: bool) -> Vec<Line> {
    // Show the date with each time only when the period spans more than one day.
    let multi_day = local_parts(zone, start).map(|p| p.0) != local_parts(zone, end).map(|p| p.0);
    let mut lines = Vec::new();
    for e in entries {
        let time = match local_parts(zone, &e.event_at_utc) {
            Some((d, t)) if multi_day => format!("{} {t}", &d[5..]),
            Some((_, t)) => t,
            None => e.event_at_utc.clone(),
        };
        lines.extend(lines_for(doc, e, time, details));
    }
    lines
}

/// The log table exactly as it is drawn: one array of six cells per printed line.
pub fn table_lines(conn: &Connection, req: &Form309Request) -> AppResult<Vec<[String; 6]>> {
    let zone = DisplayZone::parse(&req.display_zone);
    let op = get_period(conn, &req.op_id)?;
    let (entries, _) = plan(conn, req)?;
    let doc = DrawnDoc::new()?;
    Ok(log_lines(&doc, &entries, zone, &op.start_utc, &op.end_utc, req.include_details).into_iter().map(|l| l.cells).collect())
}

pub fn build_309(conn: &Connection, info: &LogbookInfo, req: &Form309Request) -> AppResult<BuiltForm> {
    require_emergency(info)?;
    if req.kinds.is_empty() {
        return Err(AppError::validation("kinds", "Choose at least one entry type to include"));
    }
    let zone = DisplayZone::parse(&req.display_zone);
    let op = get_period(conn, &req.op_id)?;
    let (entries, skipped) = plan(conn, req)?;
    let mut doc = DrawnDoc::new()?;
    let lines = log_lines(&doc, &entries, zone, &op.start_utc, &op.end_utc, req.include_details);
    let join = |a: &str, b: &str| [a.trim(), b.trim()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(", ");
    let header = Header {
        incident: info.incident.incident_name.clone(),
        from: date_time(zone, &op.start_utc),
        to: date_time(zone, &op.end_utc),
        net: req.net_name.clone(),
        operator: join(&req.operator_name, &req.operator_call),
        prepared_by: join(&req.prepared_by_name, &req.prepared_by_call),
        prepared_at: date_time(zone, &now_utc()),
        footer: format!(
            "{} \u{00b7} operational period \u{201c}{}\u{201d} \u{00b7} {} entries \u{00b7} {}",
            footer_base(info),
            op.name,
            entries.len(),
            zone_note(zone)
        ),
    };
    let chunks: Vec<&[Line]> = if lines.is_empty() { vec![&[]] } else { lines.chunks(ROWS_PER_PAGE).collect() };
    let pages = chunks.len();
    for (i, chunk) in chunks.iter().enumerate() {
        let ops = draw_page(&mut doc, &header, chunk, i + 1, pages);
        doc.add_page(ops);
    }
    let mut warnings = Vec::new();
    if entries.is_empty() {
        warnings.push("No entries matched; the log is empty.".into());
    }
    let pdf = doc.finish(&format!("ICS 309 Communications Log \u{2014} {} \u{2014} {}", info.name, op.name))?;
    Ok(BuiltForm { pdf, exported: entries.len() as i64, pages: pages as i64, skipped, warnings })
}

pub fn export_309(conn: &Connection, info: &LogbookInfo, req: &Form309Request, dest: &Path) -> AppResult<FormResult> {
    let f = build_309(conn, info, req)?;
    let bytes = publish(&f.pdf, dest)?;
    Ok(FormResult {
        path: dest.to_string_lossy().to_string(),
        form: "ics309".into(),
        exported: f.exported,
        pages: f.pages,
        skipped: f.skipped,
        warnings: f.warnings,
        bytes,
    })
}
