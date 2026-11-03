//! XLSX: "Entries" sheet with frozen header, autofilter, a typed UTC datetime column, and all
//! values as text cells (never formulas). Text that looks like a formula also gets Excel's
//! quote-prefix style, so editing the cell cannot turn it into a formula. A "Report" sheet holds
//! the export metadata.

use super::{ReportContext, columns, is_formula_like};
use crate::entries::for_each_entry;
use crate::error::{AppError, AppResult};
use crate::models::EntryQuery;
use crate::validate::parse_utc;
use chrono::{Datelike, Timelike};
use rusqlite::Connection;
use rust_xlsxwriter::{ExcelDateTime, Format, FormatAlign, Workbook, XlsxError};
use std::path::Path;

pub const MAX_CELL_CHARS: usize = 32_000;

fn xerr(e: XlsxError) -> AppError {
    match e {
        XlsxError::IoError(io) => io.into(),
        other => AppError::internal(format!("XLSX error: {other}")),
    }
}

fn excel_dt(iso: &str) -> Option<ExcelDateTime> {
    let t = parse_utc(iso)?;
    let secs = t.second() as f64 + t.nanosecond() as f64 / 1e9;
    ExcelDateTime::from_ymd(t.year() as u16, t.month() as u8, t.day() as u8).ok()?.and_hms(t.hour() as u16, t.minute() as u8, secs).ok()
}

pub fn write(conn: &Connection, q: &EntryQuery, ctx: &ReportContext, path: &Path) -> AppResult<(i64, Vec<String>)> {
    let mut wb = Workbook::new();
    let header = Format::new().set_bold().set_background_color("DDE6F0").set_text_wrap().set_align(FormatAlign::Top);
    let quoted = Format::new().set_quote_prefix();
    let dt = Format::new().set_num_format("yyyy-mm-dd hh:mm:ss");
    let cols = columns(ctx.logbook.logbook_type);
    let utc_col = cols.iter().position(|(h, _)| *h == "Event Time UTC").unwrap() as u16;
    let mut warnings = Vec::new();
    let mut n = 0i64;
    {
        let ws = wb.add_worksheet_with_constant_memory().map_err(xerr)?;
        ws.set_name("Entries").map_err(xerr)?;
        for (i, (h, _)) in cols.iter().enumerate() {
            ws.write_string_with_format(0, i as u16, *h, &header).map_err(xerr)?;
            let width = match *h {
                "Summary" => 40.0,
                "Message" => 60.0,
                "Event Time" | "Event Time UTC" | "Recorded UTC" => 22.0,
                _ => 14.0,
            };
            ws.set_column_width(i as u16, width).map_err(xerr)?;
        }
        ws.set_freeze_panes(1, 0).map_err(xerr)?;
        let mut row: u32 = 1;
        for_each_entry(conn, q, |e| {
            for (i, (h, f)) in cols.iter().enumerate() {
                let c = i as u16;
                if c == utc_col
                    && let Some(d) = excel_dt(&e.event_at_utc)
                {
                    ws.write_datetime_with_format(row, c, &d, &dt).map_err(xerr)?;
                    continue;
                }
                let mut v = f(e, ctx.zone);
                if v.is_empty() {
                    continue;
                }
                if v.chars().count() > MAX_CELL_CHARS {
                    v = v.chars().take(MAX_CELL_CHARS).collect::<String>();
                    v.push_str(" [...truncated: Excel cell limit; full text is in TXT/DOCX/PDF exports]");
                    warnings.push(format!("Entry #{} {h} exceeded the Excel cell limit and was truncated in XLSX", e.sequence_no));
                }
                if is_formula_like(&v) {
                    ws.write_string_with_format(row, c, &v, &quoted).map_err(xerr)?;
                } else {
                    ws.write_string(row, c, &v).map_err(xerr)?;
                }
            }
            row += 1;
            n += 1;
            Ok(())
        })?;
        ws.autofilter(0, 0, row.saturating_sub(1), (cols.len() - 1) as u16).map_err(xerr)?;
    }
    {
        let ws = wb.add_worksheet_with_constant_memory().map_err(xerr)?;
        ws.set_name("Report").map_err(xerr)?;
        ws.set_column_width(0, 18.0).map_err(xerr)?;
        ws.set_column_width(1, 90.0).map_err(xerr)?;
        let rows = [
            ("Report", "EMCOMM LOG Export".to_string()),
            ("Logbook", ctx.logbook.name.clone()),
            ("Type", ctx.logbook.logbook_type.label().to_string()),
            ("Incident", ctx.logbook.incident.incident_name.clone()),
            ("Generated", ctx.generated.clone()),
            ("Times shown in", ctx.zone.label()),
            ("Scope", ctx.description.clone()),
            ("Records", ctx.count.to_string()),
            ("Event Time UTC", "Typed Excel date/time cells in UTC (no time zone stored by Excel).".into()),
            (
                "Text cells",
                "All values are stored as text. Text beginning with = + - @ uses Excel's quote-prefix so it is never evaluated.".into(),
            ),
        ];
        for (i, (k, v)) in rows.iter().enumerate() {
            ws.write_string_with_format(i as u32, 0, *k, &header).map_err(xerr)?;
            if is_formula_like(v) {
                ws.write_string_with_format(i as u32, 1, v, &quoted).map_err(xerr)?;
            } else {
                ws.write_string(i as u32, 1, v).map_err(xerr)?;
            }
        }
    }
    wb.save(path).map_err(xerr)?;
    std::fs::File::open(path)?.sync_all()?;
    Ok((n, warnings))
}
