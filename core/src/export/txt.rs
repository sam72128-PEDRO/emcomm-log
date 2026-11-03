//! TXT: UTF-8 human-readable chronological log with full message text.

use super::{detail_lines, heading_line, ReportContext};
use crate::entries::for_each_entry;
use crate::error::AppResult;
use crate::models::EntryQuery;
use rusqlite::Connection;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

const RULE: &str = "================================================================================";
const SEP: &str = "--------------------------------------------------------------------------------";

pub fn write(conn: &Connection, q: &EntryQuery, ctx: &ReportContext, path: &Path) -> AppResult<i64> {
    let mut w = BufWriter::new(File::create(path)?);
    let lb = &ctx.logbook;
    writeln!(w, "EMCOMM LOG - {} ({} logbook)", lb.name, lb.logbook_type.label())?;
    if !lb.incident.incident_name.is_empty() {
        writeln!(
            w,
            "Incident: {}{}{}",
            lb.incident.incident_name,
            if lb.incident.incident_number.is_empty() { String::new() } else { format!(" #{}", lb.incident.incident_number) },
            if lb.incident.operational_period.is_empty() { String::new() } else { format!("  Op period: {}", lb.incident.operational_period) }
        )?;
    }
    writeln!(w, "Generated: {}", ctx.generated)?;
    writeln!(w, "Times shown in: {}", ctx.zone.label())?;
    writeln!(w, "Scope: {}", ctx.description)?;
    writeln!(w, "Records: {}", ctx.count)?;
    writeln!(w, "{RULE}")?;
    let mut n = 0i64;
    for_each_entry(conn, q, |e| {
        writeln!(w, "{}", heading_line(e, ctx.zone))?;
        writeln!(w, "Summary: {}", e.subject)?;
        for (k, v) in detail_lines(e, lb.logbook_type, ctx.zone) {
            if v.contains('\n') {
                writeln!(w, "{k}:")?;
                for line in v.lines() {
                    writeln!(w, "    {line}")?;
                }
            } else {
                writeln!(w, "{k}: {v}")?;
            }
        }
        if !e.body.is_empty() {
            writeln!(w, "Message:")?;
            for line in e.body.lines() {
                writeln!(w, "    {line}")?;
            }
        }
        writeln!(w, "{SEP}")?;
        n += 1;
        Ok(())
    })?;
    writeln!(w, "End of report - {n} record(s).")?;
    w.flush()?;
    w.into_inner().map_err(|e| crate::error::AppError::from(e.into_error()))?.sync_all()?;
    Ok(n)
}
