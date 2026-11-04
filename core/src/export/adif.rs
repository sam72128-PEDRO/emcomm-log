//! ADIF 3.1.8 `.adi` export of qualifying HAM QSOs only. Missing QSO data is never invented:
//! entries without a callsign, UTC start, or band/frequency are skipped with a reason.

use super::{ReportContext, SkippedRecord};
use crate::entries::for_each_entry;
use crate::error::{AppError, AppResult};
use crate::models::{Entry, EntryQuery, EntryStatus};
use crate::validate::{band_for_hz, format_mhz, parse_utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

pub const ADIF_VERSION: &str = "3.1.8";

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

/// One canonical ADIF writer; a preset only changes which records qualify and which activation fields
/// are written (2.1 spec §6). No upload, account or site API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AdifPreset {
    /// Any logbook service (QRZ, LoTW via TQSL, Club Log…).
    #[default]
    General,
    PotaActivator,
    SotaActivator,
    SotaChaser,
}

/// What an ADIF export will contain, shown before exporting.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AdifSummary {
    pub eligible: i64,
    pub skipped: Vec<SkippedRecord>,
    pub station_callsigns: Vec<String>,
    pub operators: Vec<String>,
    /// Activation references that will be written (parks/summits).
    pub refs: Vec<String>,
    pub first_utc: Option<String>,
    pub last_utc: Option<String>,
}

/// An entry qualifies when it has a counterpart callsign, a UTC start, and band or frequency; presets
/// add their own requirements. The reason is shown to the operator; nothing is dropped silently.
pub fn eligibility_for(e: &Entry, preset: AdifPreset) -> Result<(), String> {
    eligibility(e)?;
    let h = e.ham.as_ref().expect("checked");
    let own_call = !h.station_call.is_empty() || !h.operator_call.is_empty();
    let mode = resolve_mode(&h.mode, &h.submode).0.is_some();
    match preset {
        AdifPreset::General => {}
        AdifPreset::PotaActivator => {
            if h.my_pota_ref.is_empty() {
                return Err("no activator park reference (My POTA)".into());
            }
            if !own_call {
                return Err("no station or operator callsign (POTA needs STATION_CALLSIGN or OPERATOR)".into());
            }
            if !mode {
                return Err("no ADIF mode (POTA needs MODE or SUBMODE)".into());
            }
        }
        AdifPreset::SotaActivator => {
            if h.my_sota_ref.is_empty() {
                return Err("no activator summit reference (My SOTA)".into());
            }
            if !own_call {
                return Err("no station or operator callsign".into());
            }
            if !mode {
                return Err("no ADIF mode".into());
            }
        }
        AdifPreset::SotaChaser => {
            if h.sota_ref.is_empty() {
                return Err("no summit worked (SOTA reference)".into());
            }
            if !mode {
                return Err("no ADIF mode".into());
            }
        }
    }
    Ok(())
}

/// ADIF Location type: "XDDD MM.MMM" (hemisphere, 3-digit degrees, minutes with 3 decimals).
pub fn adif_location(v: f64, is_lat: bool) -> Option<String> {
    let max = if is_lat { 90.0 } else { 180.0 };
    if !v.is_finite() || v.abs() > max {
        return None;
    }
    let hemi = match (is_lat, v < 0.0) {
        (true, false) => 'N',
        (true, true) => 'S',
        (false, false) => 'E',
        (false, true) => 'W',
    };
    let a = v.abs();
    let mut deg = a.trunc() as u32;
    let mut min = ((a - deg as f64) * 60.0 * 1000.0).round() / 1000.0;
    if min >= 60.0 {
        deg += 1;
        min = 0.0;
    }
    Some(format!("{hemi}{deg:03} {min:06.3}"))
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
    record_with(e, AdifPreset::General, false, warnings)
}

pub fn record_with(e: &Entry, preset: AdifPreset, include_coords: bool, warnings: &mut Vec<String>) -> String {
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
    // Activation references (ADIF POTARef / SOTARef). MY_SIG/SIG carry POTA only for the POTA preset,
    // as POTA's uploader expects; the *_POTA_REF / *_SOTA_REF fields are written whenever present.
    if preset == AdifPreset::PotaActivator {
        field(&mut r, "MY_SIG", "POTA");
        field(&mut r, "MY_SIG_INFO", &h.my_pota_ref);
        if !h.pota_ref.is_empty() {
            field(&mut r, "SIG", "POTA");
            field(&mut r, "SIG_INFO", &h.pota_ref);
        }
    }
    field(&mut r, "MY_POTA_REF", &h.my_pota_ref);
    field(&mut r, "POTA_REF", &h.pota_ref);
    field(&mut r, "MY_SOTA_REF", &h.my_sota_ref);
    field(&mut r, "SOTA_REF", &h.sota_ref);
    // The station's own position only (never a reported place), and only when the operator chose to
    // include precise coordinates.
    if include_coords
        && let Some(l) = &e.operator_location
        && let (Some(lat), Some(lon)) = (l.latitude_deg, l.longitude_deg)
        && let (Some(a), Some(b)) = (adif_location(lat, true), adif_location(lon, false))
    {
        field(&mut r, "MY_LAT", &a);
        field(&mut r, "MY_LON", &b);
    }
    if !h.qso_notes.is_empty() {
        match ascii_line(&h.qso_notes) {
            Some(c) => field(&mut r, "COMMENT", &c),
            None => warnings.push(format!("Entry #{}: QSO notes contain non-ASCII text and were omitted from ADIF", e.sequence_no)),
        }
    }
    r.push_str("<EOR>\n");
    r
}

pub fn summarize(conn: &Connection, q: &EntryQuery, preset: AdifPreset) -> AppResult<AdifSummary> {
    use std::collections::BTreeSet;
    let mut s = AdifSummary::default();
    let (mut calls, mut ops, mut refs) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    for_each_entry(conn, q, |e| {
        match eligibility_for(e, preset) {
            Ok(()) => {
                let h = e.ham.as_ref().expect("eligible");
                s.eligible += 1;
                for (set, v) in [(&mut calls, &h.station_call), (&mut ops, &h.operator_call)] {
                    if !v.is_empty() {
                        set.insert(v.clone());
                    }
                }
                for v in [&h.my_pota_ref, &h.pota_ref, &h.my_sota_ref, &h.sota_ref] {
                    if !v.is_empty() {
                        refs.insert(v.clone());
                    }
                }
                let t = h.qso_start_utc.clone();
                if s.first_utc.is_none() || t < s.first_utc {
                    s.first_utc = t.clone();
                }
                if t > s.last_utc {
                    s.last_utc = t;
                }
            }
            Err(reason) => s.skipped.push(SkippedRecord { sequence_no: e.sequence_no, reason }),
        }
        Ok(())
    })?;
    s.station_callsigns = calls.into_iter().collect();
    s.operators = ops.into_iter().collect();
    s.refs = refs.into_iter().collect();
    Ok(s)
}

pub fn write(
    conn: &Connection,
    q: &EntryQuery,
    ctx: &ReportContext,
    path: &Path,
    preset: AdifPreset,
    include_coords: bool,
) -> AppResult<(i64, Vec<SkippedRecord>, Vec<String>)> {
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
        match eligibility_for(e, preset) {
            Ok(()) => {
                w.write_all(record_with(e, preset, include_coords, &mut warnings).as_bytes())?;
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
