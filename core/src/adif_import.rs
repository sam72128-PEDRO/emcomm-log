//! ADIF 3.1.8 `.adi` import (2.2 ADIF-*). Local only: no QRZ/POTA/SOTA lookup, no network. The parser is
//! bounded (file size, records, fields, field length) and never interprets content. Every imported QSO is a
//! new entry with a new id and the next number; nothing existing is merged or overwritten.

use crate::entries::insert_new_entry;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::export::adif::resolve_mode;
use crate::fsutil::{new_uuid, now_utc};
use crate::logbook::Logbook;
use crate::models::*;
use crate::validate::{band_for_hz, validate_entry_input};
use rusqlite::{TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_RECORDS: usize = 100_000;
pub const MAX_FIELDS_PER_RECORD: usize = 500;
pub const MAX_FIELD_LEN: usize = 64 * 1024;
/// A record naming several parks is split into one QSO per park pair, up to this many.
pub const MAX_SPLIT: usize = 16;

// ---------------------------------------------------------------- parser

#[derive(Debug, Clone, PartialEq)]
pub struct AdifRecord {
    /// 1-based position in the file.
    pub index: usize,
    /// Field names upper-cased, in file order.
    pub fields: Vec<(String, String)>,
}

impl AdifRecord {
    pub fn get(&self, name: &str) -> &str {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, v)| v.trim()).unwrap_or("")
    }
}

#[derive(Debug, Clone, Default)]
pub struct Parsed {
    pub header: Vec<(String, String)>,
    pub records: Vec<AdifRecord>,
    /// Problems with the file itself (not with one QSO).
    pub warnings: Vec<String>,
    /// Record numbers that ended without <EOR> or had a broken field.
    pub broken: Vec<(usize, String)>,
}

fn find_ci(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w.eq_ignore_ascii_case(needle))
}

/// Parse ADI text. Lengths are counted in bytes as written by most programs (and by EMCOMM LOG); when a
/// byte count would split a UTF-8 character the length is read as characters instead.
pub fn parse_adi(input: &[u8]) -> AppResult<Parsed> {
    if input.len() as u64 > MAX_FILE_BYTES {
        return Err(AppError::validation("adif", "The file is larger than 64 MB; split it and import the parts"));
    }
    let mut b = input;
    if b.starts_with(&[0xEF, 0xBB, 0xBF]) {
        b = &b[3..];
    }
    let mut out = Parsed::default();
    let mut pos = 0usize;
    // A header exists when the file does not start with '<' (ADIF §).
    let first = b.iter().position(|c| !c.is_ascii_whitespace()).unwrap_or(b.len());
    let mut in_header = first < b.len() && b[first] != b'<';
    if in_header && find_ci(b, b"<EOH>").is_none() {
        return Err(AppError::validation("adif", "This does not look like an ADIF file: it has a header but no <EOH>"));
    }
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut record_no = 0usize;
    let mut field_error: Option<String> = None;
    while pos < b.len() {
        let Some(lt) = b[pos..].iter().position(|&c| c == b'<') else { break };
        let tag_start = pos + lt + 1;
        let Some(gt) = b[tag_start..].iter().take(256).position(|&c| c == b'>') else {
            out.warnings.push(format!("A '<' at byte {} is not a tag; ignored", pos + lt));
            pos = tag_start;
            continue;
        };
        let tag = String::from_utf8_lossy(&b[tag_start..tag_start + gt]).to_string();
        let data_start = tag_start + gt + 1;
        let mut parts = tag.splitn(3, ':');
        let name = parts.next().unwrap_or("").trim().to_ascii_uppercase();
        let len_s = parts.next();
        pos = data_start;
        match len_s {
            None => match name.as_str() {
                "EOH" => {
                    if in_header {
                        out.header = std::mem::take(&mut fields);
                        in_header = false;
                    }
                }
                "EOR" => {
                    if in_header {
                        out.warnings.push("<EOR> inside the header; ignored".into());
                        continue;
                    }
                    record_no += 1;
                    if record_no > MAX_RECORDS {
                        return Err(AppError::validation("adif", format!("More than {MAX_RECORDS} QSOs; split the file")));
                    }
                    let f = std::mem::take(&mut fields);
                    if let Some(e) = field_error.take() {
                        out.broken.push((record_no, e));
                    } else if f.is_empty() {
                        record_no -= 1; // stray <EOR>
                    } else {
                        out.records.push(AdifRecord { index: record_no, fields: f });
                    }
                }
                _ => out.warnings.push(format!("Unknown tag <{tag}> ignored")),
            },
            Some(l) => {
                let Ok(len) = l.trim().parse::<usize>() else {
                    field_error.get_or_insert_with(|| format!("Field <{tag}> has no valid length"));
                    continue;
                };
                if len > MAX_FIELD_LEN {
                    return Err(AppError::validation("adif", format!("Field {name} is longer than 64 KB")));
                }
                let end = data_start + len;
                let value = if end > b.len() {
                    field_error.get_or_insert_with(|| format!("Field {name} runs past the end of the file"));
                    pos = b.len();
                    continue;
                } else if let Ok(s) = std::str::from_utf8(&b[data_start..end]) {
                    pos = end;
                    s.to_string()
                } else {
                    // Length counted in characters.
                    let rest = String::from_utf8_lossy(&b[data_start..b.len().min(data_start + len * 4)]);
                    let s: String = rest.chars().take(len).collect();
                    pos = data_start + s.len();
                    s
                };
                if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    field_error.get_or_insert_with(|| format!("Field name '{name}' is not valid"));
                    continue;
                }
                if fields.len() >= MAX_FIELDS_PER_RECORD {
                    field_error.get_or_insert_with(|| "Too many fields in one record".into());
                    continue;
                }
                fields.push((name, value));
            }
        }
    }
    if !in_header {
        if let Some(e) = field_error {
            out.broken.push((record_no + 1, e));
        } else if !fields.is_empty() {
            out.broken.push((record_no + 1, "The last record has no <EOR> (file cut short?)".into()));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- mapping

/// Callsigns to use when a record has none (confirmed by the operator in the preview).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct ImportOptions {
    pub default_station_call: String,
    pub default_operator_call: String,
}

/// Fields that become part of the entry; everything else is kept as extras.
const MAPPED: &[&str] = &[
    "CALL",
    "QSO_DATE",
    "TIME_ON",
    "QSO_DATE_OFF",
    "TIME_OFF",
    "FREQ",
    "BAND",
    "MODE",
    "SUBMODE",
    "RST_SENT",
    "RST_RCVD",
    "STATION_CALLSIGN",
    "OPERATOR",
    "MY_GRIDSQUARE",
    "GRIDSQUARE",
    "COMMENT",
    "NOTES",
    "MY_POTA_REF",
    "POTA_REF",
    "MY_SOTA_REF",
    "SOTA_REF",
    "MY_SIG",
    "MY_SIG_INFO",
    "SIG",
    "SIG_INFO",
];

#[derive(Debug, Clone)]
pub struct Mapped {
    pub key: String,
    pub input: EntryInput,
    pub extras: BTreeMap<String, String>,
    pub warnings: Vec<String>,
    pub record_sha256: String,
}

fn utc(date: &str, time: &str, field: &str) -> Result<chrono::DateTime<chrono::Utc>, String> {
    let d = chrono::NaiveDate::parse_from_str(date, "%Y%m%d").map_err(|_| format!("{field} date '{date}' is not YYYYMMDD"))?;
    let t = match time.len() {
        4 => chrono::NaiveTime::parse_from_str(time, "%H%M"),
        6 => chrono::NaiveTime::parse_from_str(time, "%H%M%S"),
        _ => return Err(format!("{field} time '{time}' is not HHMM or HHMMSS")),
    }
    .map_err(|_| format!("{field} time '{time}' is not a valid time"))?;
    Ok(chrono::DateTime::from_naive_utc_and_offset(d.and_time(t), chrono::Utc))
}

fn iso(t: chrono::DateTime<chrono::Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn refs(list: &str) -> Vec<String> {
    list.split(',').map(|s| s.trim().to_uppercase()).filter(|s| !s.is_empty()).collect()
}

fn record_hash(r: &AdifRecord) -> String {
    let mut f: Vec<(String, String)> = r.fields.iter().map(|(n, v)| (n.clone(), v.trim().to_string())).collect();
    f.sort();
    let mut h = Sha256::new();
    for (n, v) in f {
        h.update(n.as_bytes());
        h.update(b"=");
        h.update(v.as_bytes());
        h.update(b"\n");
    }
    hex::encode(h.finalize())
}

/// Map one record to one or more QSO entries (several when it names several parks), validated with the
/// same rules as Save Entry. Err = why it cannot be imported.
pub fn map_record(r: &AdifRecord, opts: &ImportOptions) -> Result<Vec<Mapped>, String> {
    let call = r.get("CALL");
    if call.is_empty() {
        return Err("No CALL (the other station's callsign)".into());
    }
    let (date, time_on) = (r.get("QSO_DATE"), r.get("TIME_ON"));
    if date.is_empty() || time_on.is_empty() {
        return Err("No QSO_DATE / TIME_ON (UTC start)".into());
    }
    let start = utc(date, time_on, "QSO_DATE/TIME_ON")?;
    let end = match r.get("TIME_OFF") {
        "" => None,
        t_off => {
            let d_off = match r.get("QSO_DATE_OFF") {
                "" => date,
                d => d,
            };
            let mut e = utc(d_off, t_off, "TIME_OFF")?;
            if r.get("QSO_DATE_OFF").is_empty() && e < start {
                e += chrono::Duration::days(1); // ended after UTC midnight
            }
            Some(e)
        }
    };
    let mut warnings = Vec::new();
    let freq = match r.get("FREQ") {
        "" => None,
        f => {
            let mhz: f64 = f.parse().map_err(|_| format!("FREQ '{f}' is not a number of MHz"))?;
            if !(mhz > 0.0 && mhz.is_finite()) {
                return Err(format!("FREQ '{f}' is not a valid frequency"));
            }
            Some((mhz * 1e6).round() as i64)
        }
    };
    let mut band = r.get("BAND").to_ascii_lowercase();
    if band.is_empty()
        && let Some(hz) = freq
        && let Some(b) = band_for_hz(hz)
    {
        band = b.to_string();
    }
    let mut notes = r.get("COMMENT").to_string();
    if !r.get("NOTES").is_empty() {
        if !notes.is_empty() {
            notes.push('\n');
        }
        notes.push_str(r.get("NOTES"));
    }
    // POTA: *_POTA_REF, or MY_SIG/SIG = POTA with *_SIG_INFO. Other programs (WWFF, IOTA…) are kept as extras.
    let sig_is = |n: &str, p: &str| r.get(n).eq_ignore_ascii_case(p);
    let mut my_pota = refs(r.get("MY_POTA_REF"));
    if my_pota.is_empty() && sig_is("MY_SIG", "POTA") {
        my_pota = refs(r.get("MY_SIG_INFO"));
    }
    let mut pota = refs(r.get("POTA_REF"));
    if pota.is_empty() && sig_is("SIG", "POTA") {
        pota = refs(r.get("SIG_INFO"));
    }
    let mut my_sota = r.get("MY_SOTA_REF").to_string();
    if my_sota.is_empty() && sig_is("MY_SIG", "SOTA") {
        my_sota = r.get("MY_SIG_INFO").to_string();
    }
    let mut sota = r.get("SOTA_REF").to_string();
    if sota.is_empty() && sig_is("SIG", "SOTA") {
        sota = r.get("SIG_INFO").to_string();
    }
    let mine: Vec<String> = if my_pota.is_empty() { vec![String::new()] } else { my_pota.clone() };
    let theirs: Vec<String> = if pota.is_empty() { vec![String::new()] } else { pota.clone() };
    if mine.len() * theirs.len() > MAX_SPLIT {
        return Err(format!("Names {} parks; at most {MAX_SPLIT} park pairs per QSO are imported", mine.len() * theirs.len()));
    }
    if mine.len() * theirs.len() > 1 {
        warnings.push(format!("Lists several parks; imported as {} QSOs, one per park", mine.len() * theirs.len()));
    }
    let mut extras: BTreeMap<String, String> = BTreeMap::new();
    for (n, v) in &r.fields {
        let mapped = MAPPED.contains(&n.as_str())
            && !((n == "MY_SIG" || n == "MY_SIG_INFO") && !(sig_is("MY_SIG", "POTA") || sig_is("MY_SIG", "SOTA")))
            && !((n == "SIG" || n == "SIG_INFO") && !(sig_is("SIG", "POTA") || sig_is("SIG", "SOTA")));
        if !mapped && !v.trim().is_empty() {
            extras.insert(n.clone(), v.clone());
        }
    }
    let station_call = match r.get("STATION_CALLSIGN") {
        "" => opts.default_station_call.clone(),
        s => s.to_string(),
    };
    let operator_call = match r.get("OPERATOR") {
        "" => opts.default_operator_call.clone(),
        s => s.to_string(),
    };
    let sha = record_hash(r);
    let mut out = Vec::new();
    for (i, mp) in mine.iter().enumerate() {
        for (j, tp) in theirs.iter().enumerate() {
            let ham = HamFields {
                their_call: call.to_string(),
                station_call: station_call.clone(),
                operator_call: operator_call.clone(),
                qso_start_utc: Some(iso(start)),
                qso_end_utc: end.map(iso),
                frequency_hz: freq,
                band: band.clone(),
                mode: r.get("MODE").to_string(),
                submode: r.get("SUBMODE").to_string(),
                rst_sent: r.get("RST_SENT").to_string(),
                rst_rcvd: r.get("RST_RCVD").to_string(),
                my_grid: r.get("MY_GRIDSQUARE").to_string(),
                their_grid: r.get("GRIDSQUARE").to_string(),
                qso_notes: notes.clone(),
                my_pota_ref: mp.clone(),
                pota_ref: tp.clone(),
                my_sota_ref: my_sota.clone(),
                sota_ref: sota.clone(),
            };
            let input = EntryInput {
                kind: EntryKind::Qso,
                event_at_utc: iso(start),
                event_timezone: "UTC".into(),
                event_utc_offset_min: 0,
                direction: Direction::Other,
                priority: Priority::Routine,
                subject: format!("QSO with {}", call.to_uppercase()),
                ham: Some(ham),
                ..Default::default()
            };
            // Same rules as Save Entry; the message names the field that is wrong.
            let v = validate_entry_input(&input, LogbookType::Ham, chrono::Utc::now()).map_err(|e| {
                let f = e.field.unwrap_or_default().replace("ham.", "");
                if f.is_empty() { e.message } else { format!("{f}: {}", e.message) }
            })?;
            out.push(Mapped {
                key: format!("{}:{}", r.index, i * theirs.len() + j),
                input: v,
                extras: extras.clone(),
                warnings: warnings.clone(),
                record_sha256: format!("{sha}:{}", i * theirs.len() + j),
            });
        }
    }
    Ok(out)
}

/// What makes two QSOs the same contact: call, UTC minute, band, mode, station callsign and the
/// park/summit context (so a two-park activation is two QSOs, not a duplicate).
pub fn dup_key(h: &HamFields) -> String {
    let minute = h.qso_start_utc.as_deref().map(|s| s.chars().take(16).collect::<String>()).unwrap_or_default();
    let band = if h.band.is_empty() { h.frequency_hz.and_then(band_for_hz).unwrap_or("").to_string() } else { h.band.clone() };
    let mode = resolve_mode(&h.mode, &h.submode).0.unwrap_or_else(|| h.mode.clone());
    [h.their_call.as_str(), &minute, &band, &mode, &h.station_call, &h.my_pota_ref, &h.pota_ref, &h.my_sota_ref, &h.sota_ref]
        .join("|")
        .to_uppercase()
}

// ---------------------------------------------------------------- preview and import

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RowStatus {
    Ok,
    Duplicate,
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PreviewRow {
    /// Stable selection key "record:part".
    pub key: String,
    pub record: usize,
    pub status: RowStatus,
    pub reason: String,
    pub warnings: Vec<String>,
    pub call: String,
    pub start_utc: Option<String>,
    pub band: String,
    pub frequency_hz: Option<i64>,
    pub mode: String,
    pub station_call: String,
    pub operator_call: String,
    pub my_pota_ref: String,
    pub pota_ref: String,
    pub my_sota_ref: String,
    pub sota_ref: String,
    /// Entry number already in the logbook that looks like the same contact.
    pub duplicate_of: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PreviousBatch {
    pub id: String,
    pub source_name: String,
    pub imported_at_utc: String,
    pub imported: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AdifPreview {
    pub source_name: String,
    pub source_sha256: String,
    pub size_bytes: u64,
    pub program: String,
    pub adif_version: String,
    pub records: usize,
    pub rows: Vec<PreviewRow>,
    pub ok: usize,
    pub duplicates: usize,
    pub invalid: usize,
    /// Fields kept as extras on each QSO (name, how many QSOs have it).
    pub kept_as_extras: Vec<(String, usize)>,
    pub warnings: Vec<String>,
    /// Earlier imports of this exact file.
    pub previous_batches: Vec<PreviousBatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AdifImportResult {
    pub batch_id: String,
    pub imported: usize,
    pub skipped: usize,
    pub first_sequence: Option<i64>,
    pub last_sequence: Option<i64>,
}

struct Analysis {
    sha: String,
    parsed: Parsed,
    rows: Vec<PreviewRow>,
    mapped: HashMap<String, Mapped>,
}

impl Logbook {
    fn analyse_adif(&self, bytes: &[u8], opts: &ImportOptions) -> AppResult<Analysis> {
        if self.logbook_type != LogbookType::Ham {
            return Err(AppError::validation("adif", "ADIF import is for HAM logbooks"));
        }
        let sha = hex::encode(Sha256::digest(bytes));
        let parsed = parse_adi(bytes)?;
        // Existing QSOs (active) by contact key, and every record already imported from any file.
        let mut existing: HashMap<String, i64> = HashMap::new();
        {
            let mut st = self.conn.prepare(
                "SELECT e.sequence_no, h.their_call, h.station_call, h.qso_start_utc, h.frequency_hz, h.band, h.mode, h.submode,
                        h.my_pota_ref, h.pota_ref, h.my_sota_ref, h.sota_ref
                 FROM entries e JOIN entry_ham h ON h.entry_id = e.id WHERE e.status = 'active' AND e.kind = 'qso'",
            )?;
            let rows = st.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    HamFields {
                        their_call: r.get(1)?,
                        station_call: r.get(2)?,
                        qso_start_utc: r.get(3)?,
                        frequency_hz: r.get(4)?,
                        band: r.get(5)?,
                        mode: r.get(6)?,
                        submode: r.get(7)?,
                        my_pota_ref: r.get(8)?,
                        pota_ref: r.get(9)?,
                        my_sota_ref: r.get(10)?,
                        sota_ref: r.get(11)?,
                        ..Default::default()
                    },
                ))
            })?;
            for row in rows {
                let (seq, h) = row?;
                existing.entry(dup_key(&h)).or_insert(seq);
            }
        }
        let mut imported_hashes: HashMap<String, i64> = HashMap::new();
        {
            let mut st = self.conn.prepare(
                "SELECT s.record_sha256, e.sequence_no FROM entry_import_source s JOIN entries e ON e.id = s.entry_id
                 WHERE e.status = 'active'",
            )?;
            for row in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
                let (h, seq) = row?;
                imported_hashes.insert(h, seq);
            }
        }
        let mut rows = Vec::new();
        let mut mapped_all = HashMap::new();
        let mut seen_in_file: HashSet<String> = HashSet::new();
        for (rec, why) in &parsed.broken {
            rows.push(blank_row(format!("{rec}:0"), *rec, RowStatus::Invalid, why.clone()));
        }
        for r in &parsed.records {
            match map_record(r, opts) {
                Err(why) => {
                    let mut row = blank_row(format!("{}:0", r.index), r.index, RowStatus::Invalid, why);
                    row.call = r.get("CALL").to_uppercase();
                    rows.push(row);
                }
                Ok(ms) => {
                    for m in ms {
                        let h = m.input.ham.as_ref().expect("QSO");
                        let key = dup_key(h);
                        let (status, reason, dup) = if let Some(seq) = imported_hashes.get(&m.record_sha256) {
                            (RowStatus::Duplicate, format!("Already imported from a file (entry #{seq})"), Some(*seq))
                        } else if let Some(seq) = existing.get(&key) {
                            (
                                RowStatus::Duplicate,
                                format!("Looks like entry #{seq} (same call, minute, band, mode, station and park/summit)"),
                                Some(*seq),
                            )
                        } else if !seen_in_file.insert(key) {
                            (RowStatus::Duplicate, "Same contact appears earlier in this file".into(), None)
                        } else {
                            (RowStatus::Ok, String::new(), None)
                        };
                        rows.push(PreviewRow {
                            key: m.key.clone(),
                            record: r.index,
                            status,
                            reason,
                            warnings: m.warnings.clone(),
                            call: h.their_call.clone(),
                            start_utc: h.qso_start_utc.clone(),
                            band: h.band.clone(),
                            frequency_hz: h.frequency_hz,
                            mode: if h.submode.is_empty() { h.mode.clone() } else { format!("{} {}", h.mode, h.submode) },
                            station_call: h.station_call.clone(),
                            operator_call: h.operator_call.clone(),
                            my_pota_ref: h.my_pota_ref.clone(),
                            pota_ref: h.pota_ref.clone(),
                            my_sota_ref: h.my_sota_ref.clone(),
                            sota_ref: h.sota_ref.clone(),
                            duplicate_of: dup,
                        });
                        mapped_all.insert(m.key.clone(), m);
                    }
                }
            }
        }
        rows.sort_by_key(|r| (r.record, r.key.clone()));
        Ok(Analysis { sha, parsed, rows, mapped: mapped_all })
    }

    /// Read-only preview of an ADI file: what would be imported, what looks like a duplicate, what cannot be
    /// imported and why. Nothing is written.
    pub fn adif_preview(&self, bytes: &[u8], source_name: &str, opts: &ImportOptions) -> AppResult<AdifPreview> {
        let a = self.analyse_adif(bytes, opts)?;
        let mut extras: BTreeMap<String, usize> = BTreeMap::new();
        for m in a.mapped.values() {
            for k in m.extras.keys() {
                *extras.entry(k.clone()).or_default() += 1;
            }
        }
        let hdr = |n: &str| a.parsed.header.iter().find(|(k, _)| k == n).map(|(_, v)| v.trim().to_string()).unwrap_or_default();
        let mut st = self.conn.prepare(
            "SELECT id, source_name, imported_at_utc, imported FROM adif_import_batches WHERE source_sha256=?1 ORDER BY imported_at_utc",
        )?;
        let previous_batches = st
            .query_map(params![a.sha], |r| {
                Ok(PreviousBatch { id: r.get(0)?, source_name: r.get(1)?, imported_at_utc: r.get(2)?, imported: r.get(3)? })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let count = |s: RowStatus| a.rows.iter().filter(|r| r.status == s).count();
        Ok(AdifPreview {
            source_name: crate::attach::clean_name(source_name),
            size_bytes: bytes.len() as u64,
            program: [hdr("PROGRAMID"), hdr("PROGRAMVERSION")].join(" ").trim().to_string(),
            adif_version: hdr("ADIF_VER"),
            records: a.parsed.records.len() + a.parsed.broken.len(),
            ok: count(RowStatus::Ok),
            duplicates: count(RowStatus::Duplicate),
            invalid: count(RowStatus::Invalid),
            kept_as_extras: extras.into_iter().collect(),
            warnings: a.parsed.warnings.clone(),
            previous_batches,
            source_sha256: a.sha,
            rows: a.rows,
        })
    }

    /// Import the chosen rows (by preview key) in one transaction: either every chosen QSO is saved, or none.
    /// The file is analysed again here; `expected_sha256` must match the file that was previewed.
    pub fn adif_import(
        &mut self,
        bytes: &[u8],
        source_name: &str,
        expected_sha256: &str,
        selected: &[String],
        opts: &ImportOptions,
        operator: &OperatorSnapshot,
    ) -> AppResult<AdifImportResult> {
        self.ensure_writable()?;
        let a = self.analyse_adif(bytes, opts)?;
        if a.sha != expected_sha256 {
            return Err(AppError::new(ErrorCode::Conflict, "The file changed after the preview; preview it again"));
        }
        let wanted: HashSet<&String> = selected.iter().collect();
        let chosen: Vec<&PreviewRow> = a.rows.iter().filter(|r| wanted.contains(&r.key) && r.status != RowStatus::Invalid).collect();
        if chosen.is_empty() {
            return Err(AppError::validation("adif", "No QSOs are selected for import"));
        }
        let batch_id = new_uuid();
        let now = now_utc();
        let logbook_id = self.id.clone();
        let skipped = a.rows.len() - chosen.len();
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO adif_import_batches(id, source_name, source_sha256, imported_at_utc, candidates, imported, skipped,
                summary_json, operator_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                batch_id,
                crate::attach::clean_name(source_name),
                a.sha,
                now,
                a.rows.len() as i64,
                chosen.len() as i64,
                skipped as i64,
                serde_json::to_string(&serde_json::json!({
                    "warnings": a.parsed.warnings,
                    "skipped": a.rows.iter().filter(|r| !wanted.contains(&r.key) || r.status == RowStatus::Invalid)
                        .map(|r| serde_json::json!({"record": r.record, "key": r.key, "reason": r.reason})).collect::<Vec<_>>(),
                    "options": opts,
                }))?,
                serde_json::to_string(operator)?
            ],
        )?;
        let (mut first, mut last) = (None, None);
        for row in &chosen {
            let m = &a.mapped[&row.key];
            let h = m.input.ham.as_ref().expect("QSO");
            // The imported QSO's own station callsign; the logbook's current station is not claimed for it.
            let station = StationSnapshot { station_callsign: h.station_call.clone(), ..Default::default() };
            let e = insert_new_entry(&tx, &logbook_id, &m.input, &[], operator, &new_uuid(), Some(&station))?;
            tx.execute(
                "INSERT INTO entry_import_source(entry_id, batch_id, source_index, record_sha256, extras_json) VALUES (?1,?2,?3,?4,?5)",
                params![e.id, batch_id, row.record as i64, m.record_sha256, serde_json::to_string(&m.extras)?],
            )?;
            first.get_or_insert(e.sequence_no);
            last = Some(e.sequence_no);
        }
        crate::faults::check("adif.import.before_commit")?;
        tx.commit()?;
        Ok(AdifImportResult { batch_id, imported: chosen.len(), skipped, first_sequence: first, last_sequence: last })
    }
}

fn blank_row(key: String, record: usize, status: RowStatus, reason: String) -> PreviewRow {
    PreviewRow {
        key,
        record,
        status,
        reason,
        warnings: vec![],
        call: String::new(),
        start_utc: None,
        band: String::new(),
        frequency_hz: None,
        mode: String::new(),
        station_call: String::new(),
        operator_call: String::new(),
        my_pota_ref: String::new(),
        pota_ref: String::new(),
        my_sota_ref: String::new(),
        sota_ref: String::new(),
        duplicate_of: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(n: &str, v: &str) -> String {
        format!("<{n}:{}>{v}", v.len())
    }

    #[test]
    fn parses_header_case_whitespace_unicode_and_reports_breakage() {
        let text = format!(
            "Exported by test\n<adif_ver:5>3.1.8 <programid:4>TEST <eoh>\n{}{}{}<eor>\n\n {} {} <Eor>{}",
            f("call", "W1AW"),
            f("QSO_DATE", "20261102"),
            f("TIME_ON", "2359"),
            f("CALL", "JA1AAA"),
            f("COMMENT", "ありがとう"),
            f("CALL", "N0CUT"),
        );
        let p = parse_adi(text.as_bytes()).unwrap();
        assert_eq!(p.header.iter().find(|(k, _)| k == "ADIF_VER").unwrap().1, "3.1.8");
        assert_eq!(p.records.len(), 2);
        assert_eq!(p.records[0].get("CALL"), "W1AW");
        assert_eq!(p.records[1].get("COMMENT"), "ありがとう", "byte length of UTF-8 text");
        assert_eq!(p.broken.len(), 1, "record without <EOR> reported, not dropped silently");
        // Length in characters instead of bytes.
        let chars = "<CALL:6>DL1ABC<COMMENT:5>Grüße<EOR>";
        let p = parse_adi(chars.as_bytes()).unwrap();
        assert_eq!(p.records[0].get("COMMENT"), "Grüße");
        // No header, '<' in data, a bad length and a field past the end.
        let p = parse_adi(b"<CALL:4>K1AB<NOTES:3>a<b<EOR><CALL:x>K2<EOR><CALL:99>K3").unwrap();
        assert_eq!(p.records[0].get("NOTES"), "a<b");
        assert_eq!(p.broken.len(), 2);
        assert!(parse_adi(b"header without end").is_err());
    }

    #[test]
    fn maps_utc_identities_parks_and_extras() {
        let rec = AdifRecord {
            index: 1,
            fields: [
                ("CALL", "w1aw"),
                ("QSO_DATE", "20261102"),
                ("TIME_ON", "235930"),
                ("TIME_OFF", "000130"),
                ("FREQ", "14.0620"),
                ("MODE", "SSB"),
                ("SUBMODE", "USB"),
                ("STATION_CALLSIGN", "W4CLB"),
                ("OPERATOR", "KJ4AAA"),
                ("MY_SIG", "POTA"),
                ("MY_SIG_INFO", "US-0005,US-0006"),
                ("SIG", "WWFF"),
                ("SIG_INFO", "KFF-1234"),
                ("NAME", "Hiram"),
            ]
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect(),
        };
        let m = map_record(&rec, &ImportOptions::default()).unwrap();
        assert_eq!(m.len(), 2, "two parks -> two QSOs");
        let h = m[0].input.ham.as_ref().unwrap();
        assert_eq!(h.their_call, "W1AW");
        assert_eq!((h.station_call.as_str(), h.operator_call.as_str()), ("W4CLB", "KJ4AAA"));
        assert_eq!(h.qso_start_utc.as_deref(), Some("2026-11-02T23:59:30.000Z"));
        assert_eq!(h.qso_end_utc.as_deref(), Some("2026-11-03T00:01:30.000Z"), "ends after UTC midnight");
        assert_eq!((h.band.as_str(), h.frequency_hz), ("20m", Some(14_062_000)));
        assert_eq!(m[1].input.ham.as_ref().unwrap().my_pota_ref, "US-0006");
        assert_eq!(m[0].input.event_at_utc, "2026-11-02T23:59:30.000Z", "event time is the contact, not the import");
        assert_eq!(m[0].extras.get("SIG").map(String::as_str), Some("WWFF"), "non-POTA SIG kept as extra");
        assert_eq!(m[0].extras.get("NAME").map(String::as_str), Some("Hiram"));
        assert_ne!(dup_key(h), dup_key(m[1].input.ham.as_ref().unwrap()), "different park, different contact");
        // Defaults fill only missing identities.
        let mut r2 = rec.clone();
        r2.fields.retain(|(n, _)| n != "STATION_CALLSIGN" && n != "OPERATOR");
        let m2 = map_record(&r2, &ImportOptions { default_station_call: "K4XX".into(), default_operator_call: String::new() }).unwrap();
        let h2 = m2[0].input.ham.as_ref().unwrap();
        assert_eq!((h2.station_call.as_str(), h2.operator_call.as_str()), ("K4XX", ""));
        // Readable reasons.
        let mut bad = rec.clone();
        bad.fields.retain(|(n, _)| n != "CALL");
        assert!(map_record(&bad, &ImportOptions::default()).unwrap_err().contains("CALL"));
        let mut bad = rec.clone();
        bad.fields.push(("QSO_DATE".into(), "x".into()));
        bad.fields.retain(|(n, v)| n != "QSO_DATE" || v == "x");
        assert!(map_record(&bad, &ImportOptions::default()).unwrap_err().contains("YYYYMMDD"));
        let mut bad = rec.clone();
        bad.fields.push(("BAND".into(), "40m".into()));
        assert!(map_record(&bad, &ImportOptions::default()).unwrap_err().contains("band"), "14.062 MHz is not 40m");
    }
}
