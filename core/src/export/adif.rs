//! ADIF 3.1.4 `.adi` export of qualifying HAM QSOs only. Missing QSO data is never invented:
//! entries without a callsign, UTC start, or band/frequency are skipped with a reason.

use super::{ReportContext, SkippedRecord};
use crate::entries::for_each_entry;
use crate::error::{AppError, AppResult};
use crate::models::{Entry, EntryQuery, EntryStatus};
use crate::validate::{band_for_hz, format_mhz, parse_utc};
use rusqlite::Connection;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

pub const ADIF_VERSION: &str = "3.1.4";

/// ADIF 3.1.4 Mode enumeration.
pub const MODES: &[&str] = &[
    "AM",
    "ARDOP",
    "ATV",
    "CHIP",
    "CLO",
    "CONTESTI",
    "CW",
    "DIGITALVOICE",
    "DOMINO",
    "DSTAR",
    "DYNAMIC",
    "FAX",
    "FM",
    "FSK441",
    "FT8",
    "HELL",
    "ISCAT",
    "JT4",
    "JT6M",
    "JT9",
    "JT44",
    "JT65",
    "MFSK",
    "MSK144",
    "MT63",
    "OLIVIA",
    "OPERA",
    "PAC",
    "PAX",
    "PKT",
    "PSK",
    "PSK2K",
    "Q15",
    "QRA64",
    "ROS",
    "RTTY",
    "RTTYM",
    "SSB",
    "SSTV",
    "T10",
    "THOR",
    "THRB",
    "TOR",
    "V4",
    "VOI",
    "WINMOR",
    "WSPR",
];

/// Common submodes and their parent mode.
pub const SUBMODES: &[(&str, &str)] = &[
    ("USB", "SSB"),
    ("LSB", "SSB"),
    ("FT4", "MFSK"),
    ("JS8", "MFSK"),
    ("FST4", "MFSK"),
    ("FST4W", "MFSK"),
    ("Q65", "MFSK"),
    ("FSQCALL", "MFSK"),
    ("PSK31", "PSK"),
    ("PSK63", "PSK"),
    ("PSK125", "PSK"),
    ("BPSK31", "PSK"),
    ("QPSK31", "PSK"),
    ("C4FM", "DIGITALVOICE"),
    ("DMR", "DIGITALVOICE"),
    ("FREEDV", "DIGITALVOICE"),
    ("M17", "DIGITALVOICE"),
    ("PACTOR", "PAC"),
    ("PACTOR2", "PAC"),
    ("PACTOR3", "PAC"),
    ("PACTOR4", "PAC"),
    ("VARA HF", "DYNAMIC"),
    ("VARA FM 1200", "DYNAMIC"),
    ("VARA FM 9600", "DYNAMIC"),
    ("THOR16", "THOR"),
    ("OLIVIA 8/500", "OLIVIA"),
];

/// Resolve stored mode/submode into ADIF (MODE, SUBMODE). Unknown values return None for MODE.
pub fn resolve_mode(mode: &str, submode: &str) -> (Option<String>, Option<String>) {
    let m = mode.trim().to_ascii_uppercase();
    let s = submode.trim().to_ascii_uppercase();
    if m.is_empty() && s.is_empty() {
        return (None, None);
    }
    if MODES.contains(&m.as_str()) {
        let sub = if s.is_empty() { None } else { Some(s) };
        return (Some(m), sub);
    }
    if let Some((sub, parent)) = SUBMODES.iter().find(|(sub, _)| *sub == m) {
        return (Some(parent.to_string()), Some(sub.to_string()));
    }
    (None, None)
}

/// An entry qualifies when it has a counterpart callsign, a UTC start, and band or frequency.
pub fn eligibility(e: &Entry) -> Result<(), String> {
    if e.status == EntryStatus::Void {
        return Err("voided entry".into());
    }
    let Some(h) = &e.ham else { return Err("no QSO fields (traffic entry)".into()) };
    if h.their_call.is_empty() {
        return Err("missing contacted callsign".into());
    }
    if h.qso_start_utc.is_none() {
        return Err("missing QSO start time".into());
    }
    if h.band.is_empty() && h.frequency_hz.is_none() {
        return Err("missing band and frequency".into());
    }
    Ok(())
}

fn field(out: &mut String, name: &str, value: &str) {
    if value.is_empty() {
        return;
    }
    // Length is the byte count of the data; values here are ASCII-only.
    out.push_str(&format!("<{name}:{}>{value} ", value.len()));
}

/// ADIF String fields allow ASCII 32-126 only. Returns None if anything would be lost.
fn ascii_line(s: &str) -> Option<String> {
    let flat = s.replace(['\r', '\n', '\t'], " ");
    let t = flat.trim();
    if t.chars().all(|c| (' '..='~').contains(&c)) { Some(t.to_string()) } else { None }
}

pub fn record(e: &Entry, warnings: &mut Vec<String>) -> String {
    let h = e.ham.as_ref().expect("eligible entry has ham");
    let start = parse_utc(h.qso_start_utc.as_deref().unwrap()).unwrap();
    let mut r = String::new();
    field(&mut r, "CALL", &h.their_call);
    field(&mut r, "QSO_DATE", &start.format("%Y%m%d").to_string());
    field(&mut r, "TIME_ON", &start.format("%H%M%S").to_string());
    if let Some(end) = h.qso_end_utc.as_deref().and_then(parse_utc) {
        field(&mut r, "QSO_DATE_OFF", &end.format("%Y%m%d").to_string());
        field(&mut r, "TIME_OFF", &end.format("%H%M%S").to_string());
    }
    let band = if !h.band.is_empty() { Some(h.band.clone()) } else { h.frequency_hz.and_then(band_for_hz).map(String::from) };
    if let Some(b) = band {
        field(&mut r, "BAND", &b);
    }
    if let Some(hz) = h.frequency_hz {
        field(&mut r, "FREQ", &format_mhz(hz));
    }
    match resolve_mode(&h.mode, &h.submode) {
        (Some(m), sub) => {
            field(&mut r, "MODE", &m);
            if let Some(s) = sub {
                field(&mut r, "SUBMODE", &s);
            }
        }
        (None, _) if !h.mode.is_empty() => {
            warnings.push(format!("Entry #{}: mode '{}' is not an ADIF mode; MODE omitted", e.sequence_no, h.mode));
        }
        _ => {}
    }
    field(&mut r, "RST_SENT", &h.rst_sent);
    field(&mut r, "RST_RCVD", &h.rst_rcvd);
    field(&mut r, "GRIDSQUARE", &h.their_grid);
    field(&mut r, "MY_GRIDSQUARE", &h.my_grid);
    field(&mut r, "STATION_CALLSIGN", &h.station_call);
    field(&mut r, "OPERATOR", &h.operator_call);
    if !h.qso_notes.is_empty() {
        match ascii_line(&h.qso_notes) {
            Some(c) => field(&mut r, "COMMENT", &c),
            None => warnings.push(format!("Entry #{}: QSO notes contain non-ASCII text and were omitted from ADIF", e.sequence_no)),
        }
    }
    r.push_str("<EOR>\n");
    r
}

pub fn write(conn: &Connection, q: &EntryQuery, ctx: &ReportContext, path: &Path) -> AppResult<(i64, Vec<SkippedRecord>, Vec<String>)> {
    let mut w = BufWriter::new(File::create(path)?);
    let ascii_name: String = ctx.logbook.name.chars().map(|c| if (' '..='~').contains(&c) { c } else { '?' }).collect();
    let mut header = format!(
        "EMCOMM LOG ADIF export - {ascii_name}\nScope: {}\n",
        ctx.description.chars().map(|c| if (' '..='~').contains(&c) { c } else { '?' }).collect::<String>()
    );
    let mut h = String::new();
    field(&mut h, "ADIF_VER", ADIF_VERSION);
    field(&mut h, "CREATED_TIMESTAMP", &chrono::Utc::now().format("%Y%m%d %H%M%S").to_string());
    field(&mut h, "PROGRAMID", "EMCOMM LOG");
    field(&mut h, "PROGRAMVERSION", crate::logbook::APP_VERSION);
    header.push_str(&h);
    header.push_str("<EOH>\n");
    w.write_all(header.as_bytes())?;
    let mut n = 0i64;
    let mut skipped = Vec::new();
    let mut warnings = Vec::new();
    for_each_entry(conn, q, |e| {
        match eligibility(e) {
            Ok(()) => {
                w.write_all(record(e, &mut warnings).as_bytes())?;
                n += 1;
            }
            Err(reason) => skipped.push(SkippedRecord { sequence_no: e.sequence_no, reason }),
        }
        Ok(())
    })?;
    w.flush()?;
    w.into_inner().map_err(|e| AppError::from(e.into_error()))?.sync_all()?;
    Ok((n, skipped, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_resolution() {
        assert_eq!(resolve_mode("USB", ""), (Some("SSB".into()), Some("USB".into())));
        assert_eq!(resolve_mode("ft8", ""), (Some("FT8".into()), None));
        assert_eq!(resolve_mode("FT4", ""), (Some("MFSK".into()), Some("FT4".into())));
        assert_eq!(resolve_mode("WINLINKY", ""), (None, None));
    }

    #[test]
    fn field_lengths_are_byte_counts() {
        let mut s = String::new();
        field(&mut s, "CALL", "W1AW/P");
        assert_eq!(s, "<CALL:6>W1AW/P ");
    }

    #[test]
    fn ascii_line_rejects_non_ascii() {
        assert_eq!(ascii_line("hi\nthere"), Some("hi there".into()));
        assert_eq!(ascii_line("73 ünï"), None);
    }
}
