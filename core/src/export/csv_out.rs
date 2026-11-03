//! CSV: UTF-8 (with BOM for spreadsheet compatibility), RFC 4180 quoting, CRLF record ends,
//! LF inside fields. A fixed 7-line preamble carries report metadata, then the stable header.
//! Escape policy: any cell beginning with = + - @ TAB or CR gets a leading apostrophe; remove
//! exactly one leading apostrophe from such cells to recover the original value.

use super::{columns, is_formula_like, ReportContext};
use crate::entries::for_each_entry;
use crate::error::{AppError, AppResult};
use crate::models::EntryQuery;
use rusqlite::Connection;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

pub const PREAMBLE_LINES: usize = 7;

pub fn escape_cell(s: &str) -> String {
    let normalized = s.replace("\r\n", "\n").replace('\r', "\n");
    if is_formula_like(s) {
        format!("'{normalized}")
    } else {
        normalized
    }
}

fn csv_err(e: csv::Error) -> AppError {
    match e.into_kind() {
        csv::ErrorKind::Io(io) => io.into(),
        other => AppError::internal(format!("CSV error: {other:?}")),
    }
}

pub fn write(conn: &Connection, q: &EntryQuery, ctx: &ReportContext, path: &Path) -> AppResult<i64> {
    let mut f = BufWriter::new(File::create(path)?);
    f.write_all(b"\xEF\xBB\xBF")?;
    let mut w = csv::WriterBuilder::new()
        .terminator(csv::Terminator::CRLF)
        .quote_style(csv::QuoteStyle::Necessary)
        .flexible(true)
        .from_writer(f);
    let meta = [
        vec!["EMCOMM LOG Export".to_string()],
        vec!["Logbook".into(), escape_cell(&ctx.logbook.name)],
        vec!["Type".into(), ctx.logbook.logbook_type.label().into()],
        vec!["Generated".into(), ctx.generated.clone()],
        vec!["Scope".into(), escape_cell(&ctx.description)],
        vec!["Records".into(), ctx.count.to_string()],
        vec![String::new()],
    ];
    debug_assert_eq!(meta.len(), PREAMBLE_LINES);
    for row in meta {
        w.write_record(&row).map_err(csv_err)?;
    }
    let cols = columns(ctx.logbook.logbook_type);
    w.write_record(cols.iter().map(|(h, _)| *h)).map_err(csv_err)?;
    let mut n = 0i64;
    for_each_entry(conn, q, |e| {
        w.write_record(cols.iter().map(|(_, f)| escape_cell(&f(e, ctx.zone)))).map_err(csv_err)?;
        n += 1;
        Ok(())
    })?;
    let mut inner = w.into_inner().map_err(|e| AppError::from(e.into_error()))?;
    inner.flush()?;
    inner.into_inner().map_err(|e| AppError::from(e.into_error()))?.sync_all()?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutralizes_formula_triggers() {
        assert_eq!(escape_cell("=SUM(A1)"), "'=SUM(A1)");
        assert_eq!(escape_cell("@cmd"), "'@cmd");
        assert_eq!(escape_cell("plain"), "plain");
        assert_eq!(escape_cell("a\r\nb"), "a\nb");
    }
}
