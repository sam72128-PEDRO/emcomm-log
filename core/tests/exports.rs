//! Report export tests (A11, A12). Set EMCOMM_FIXTURE_OUT=<dir> to also write the fixture
//! files for independent validation (scripts/validate_exports.py).

mod common;

use common::*;
use emcomm_core::export::{self, ExportFormat, ExportRequest, adif, csv_out};
use emcomm_core::logbook::Logbook;
use emcomm_core::models::*;
use std::io::Read;
use std::path::{Path, PathBuf};

const LONG: usize = 6000;

fn long_body() -> String {
    let mut s = String::from("START-OF-LONG-MESSAGE ");
    while s.len() < LONG {
        s.push_str("Shelter-1 reports 68 occupants, 4 pets, generator at 40% — ünïcödé ✓ 日本. ");
    }
    s.push_str("\nSecond paragraph line.\nEND-OF-LONG-MESSAGE");
    s
}

/// General book: 5 entries over two local days, one voided, one formula-like, one long.
fn general_fixture(parent: &Path) -> (PathBuf, Logbook) {
    let dir = create_book(parent, LogbookType::General, "Export Fixture ✓");
    let mut lb = Logbook::open(&dir).unwrap();
    let times = ["2026-11-01T13:00:00Z", "2026-11-01T23:30:00Z", "2026-11-02T01:15:00Z", "2026-11-02T15:00:00Z", "2026-11-02T16:00:00Z"];
    let mut ids = vec![];
    for (i, t) in times.iter().enumerate() {
        let mut e = input(&format!("Entry {i}"));
        e.event_at_utc = (*t).into();
        e.event_timezone = "UTC".into();
        e.event_utc_offset_min = 0;
        if i == 1 {
            e.subject = "=HYPERLINK(\"http://evil\",\"click\")".into();
            e.from_text = "+cmd".into();
        }
        if i == 2 {
            e.body = long_body();
        }
        ids.push(lb.save_new(&e, &rid(), &operator()).unwrap().entry.id);
    }
    lb.void_entry(&ids[3], "duplicate", &rid(), &operator()).unwrap();
    (dir, lb)
}

fn ham_fixture(parent: &Path) -> (PathBuf, Logbook) {
    let dir = create_book(parent, LogbookType::Ham, "HAM Fixture");
    let mut lb = Logbook::open(&dir).unwrap();
    lb.save_new(&qso("W1AW", "2026-11-02T19:00:00Z", Some(14_074_000), "20m", "FT8"), &rid(), &operator()).unwrap();
    lb.save_new(&qso("K1ABC/P", "2026-11-02T19:05:00Z", Some(7_200_000), "", "LSB"), &rid(), &operator()).unwrap();
    let mut notes = qso("VE3XYZ", "2026-11-02T19:10:00Z", None, "2m", "FM");
    notes.ham.as_mut().unwrap().qso_notes = "Good signal\nsecond line".into();
    notes.ham.as_mut().unwrap().qso_end_utc = Some("2026-11-02T19:12:30Z".into());
    lb.save_new(&notes, &rid(), &operator()).unwrap();
    let mut uni = qso("JA1AAA", "2026-11-02T19:20:00Z", Some(21_074_000), "", "WEIRDMODE");
    uni.ham.as_mut().unwrap().qso_notes = "ありがとう".into();
    lb.save_new(&uni, &rid(), &operator()).unwrap();
    let mut traffic = input("Net check-in, no QSO data");
    traffic.kind = EntryKind::Traffic;
    lb.save_new(&traffic, &rid(), &operator()).unwrap();
    let v = lb.save_new(&qso("N0VOID", "2026-11-02T19:30:00Z", Some(14_200_000), "20m", "SSB"), &rid(), &operator()).unwrap();
    lb.void_entry(&v.entry.id, "bad call", &rid(), &operator()).unwrap();
    (dir, lb)
}

fn req(format: ExportFormat, scope: &str) -> ExportRequest {
    ExportRequest {
        format,
        scope: scope.into(),
        range_from: None,
        range_to: None,
        filter: None,
        include_voided: false,
        display_zone: "utc".into(),
    }
}

fn unzip_text(path: &Path, member: &str) -> String {
    let mut a = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut s = String::new();
    a.by_name(member).unwrap().read_to_string(&mut s).unwrap();
    s
}

#[test]
fn a11_all_report_formats_match_scope_order_and_void_rules() {
    let parent = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let (_dir, lb) = general_fixture(parent.path());
    let info = lb.info().unwrap();
    for f in [ExportFormat::Csv, ExportFormat::Txt, ExportFormat::Xlsx, ExportFormat::Docx, ExportFormat::Pdf] {
        let pv = export::preview(&lb.conn, info.logbook_type, &req(f, "all")).unwrap();
        assert_eq!(pv.count, 4, "voided excluded by default");
        let dest = out.path().join(format!("all.{}", f.extension()));
        let r = export::run_export(&lb.conn, &info, &req(f, "all"), &dest).unwrap();
        assert_eq!((r.count, r.exported), (4, 4), "{f:?}");
        let mut with_void = req(f, "all");
        with_void.include_voided = true;
        let r2 = export::run_export(&lb.conn, &info, &with_void, &out.path().join(format!("void.{}", f.extension()))).unwrap();
        assert_eq!(r2.exported, 5);
    }
    // CSV: parse independently, check stable header, order, escaping, and no truncation.
    let raw = std::fs::read(out.path().join("all.csv")).unwrap();
    assert!(raw.starts_with(b"\xEF\xBB\xBF"));
    let mut rdr = csv::ReaderBuilder::new().has_headers(false).flexible(true).from_reader(&raw[3..]);
    let rows: Vec<csv::StringRecord> = rdr.records().map(|r| r.unwrap()).collect();
    let header = &rows[csv_out::PREAMBLE_LINES];
    let expected: Vec<&str> = export::columns(LogbookType::General).iter().map(|(h, _)| *h).collect();
    assert_eq!(header.iter().collect::<Vec<_>>(), expected);
    let data = &rows[csv_out::PREAMBLE_LINES + 1..];
    assert_eq!(data.len(), 4);
    let seqs: Vec<&str> = data.iter().map(|r| r.get(0).unwrap()).collect();
    assert_eq!(seqs, ["1", "2", "3", "5"], "event-time order, voided #4 excluded");
    assert_eq!(data[1].get(13).unwrap(), "'=HYPERLINK(\"http://evil\",\"click\")");
    assert_eq!(data[1].get(11).unwrap(), "'+cmd");
    let body = data[2].get(14).unwrap();
    assert!(body.starts_with("START-OF-LONG-MESSAGE") && body.ends_with("END-OF-LONG-MESSAGE"));
    assert!(body.contains("ünïcödé ✓ 日本"));
    // TXT keeps full text.
    let txt = std::fs::read_to_string(out.path().join("all.txt")).unwrap();
    assert!(txt.contains("END-OF-LONG-MESSAGE") && txt.contains("Records: 4") && !txt.contains("#0004"));
    // DOCX contains full long text (XML-escaped) and every record heading.
    let doc = unzip_text(&out.path().join("all.docx"), "word/document.xml");
    for s in ["#0001", "#0002", "#0003", "#0005", "END-OF-LONG-MESSAGE", "=HYPERLINK(&quot;http://evil&quot;"] {
        assert!(doc.contains(s), "docx missing {s}");
    }
    assert!(!doc.contains("#0004"));
    // XLSX has header + 4 rows and quote-prefix for formula-like text.
    let sheet = unzip_text(&out.path().join("all.xlsx"), "xl/worksheets/sheet1.xml");
    assert_eq!(sheet.matches("<row ").count(), 5);
    assert!(sheet.contains("autoFilter") && sheet.contains("<pane"));
    let styles = unzip_text(&out.path().join("all.xlsx"), "xl/styles.xml");
    assert!(styles.contains("quotePrefix=\"1\""));
    // PDF is structurally complete.
    let pdf = std::fs::read(out.path().join("all.pdf")).unwrap();
    assert!(pdf.starts_with(b"%PDF-1.7") && pdf.ends_with(b"%%EOF\n"));
    let s = String::from_utf8_lossy(&pdf);
    assert!(s.contains("/ToUnicode") && s.contains("/Type /Pages"));
}

#[test]
fn a11_range_and_filter_scopes() {
    let parent = tempfile::tempdir().unwrap();
    let (_dir, lb) = general_fixture(parent.path());
    let info = lb.info().unwrap();
    let mut r = req(ExportFormat::Csv, "range");
    r.range_from = Some("2026-11-02".into());
    r.range_to = Some("2026-11-02".into());
    // UTC zone: 01:15 and 16:00 on Nov 2 (15:00 is voided).
    assert_eq!(export::preview(&lb.conn, info.logbook_type, &r).unwrap().count, 2);
    r.include_voided = true;
    assert_eq!(export::preview(&lb.conn, info.logbook_type, &r).unwrap().count, 3);
    r.range_from = Some("2026-11-03".into());
    r.range_to = Some("2026-11-02".into());
    assert!(export::preview(&lb.conn, info.logbook_type, &r).is_err());
    let mut f = req(ExportFormat::Txt, "filter");
    f.filter = Some(EntryQuery { text: "long".into(), ..Default::default() });
    let pv = export::preview(&lb.conn, info.logbook_type, &f).unwrap();
    assert_eq!(pv.count, 1);
    assert!(pv.description.contains("text \"long\""));
}

/// Minimal independent ADI reader: returns records as name->value maps, verifying that every
/// declared length matches the data exactly.
fn parse_adi(s: &str) -> Vec<std::collections::HashMap<String, String>> {
    let bytes = s.as_bytes();
    let eoh = s.to_uppercase().find("<EOH>").unwrap() + 5;
    let mut i = eoh;
    let mut recs = vec![];
    let mut cur = std::collections::HashMap::new();
    while let Some(off) = s[i..].find('<') {
        let start = i + off + 1;
        let end = start + s[start..].find('>').unwrap();
        let spec = &s[start..end];
        if spec.eq_ignore_ascii_case("EOR") {
            recs.push(std::mem::take(&mut cur));
            i = end + 1;
            continue;
        }
        let mut parts = spec.split(':');
        let name = parts.next().unwrap().to_uppercase();
        let len: usize = parts.next().unwrap().parse().unwrap();
        let val = std::str::from_utf8(&bytes[end + 1..end + 1 + len]).unwrap().to_string();
        assert!(val.chars().all(|c| (' '..='~').contains(&c)), "ADI value must be ASCII: {name}={val}");
        cur.insert(name, val);
        i = end + 1 + len;
    }
    recs
}

#[test]
fn a12_adif_contains_only_eligible_qsos_with_valid_fields() {
    let parent = tempfile::tempdir().unwrap();
    let (_dir, lb) = ham_fixture(parent.path());
    let info = lb.info().unwrap();
    let mut r = req(ExportFormat::Adif, "all");
    r.include_voided = true;
    let pv = export::preview(&lb.conn, info.logbook_type, &r).unwrap();
    assert_eq!((pv.adif_eligible, pv.adif_skipped), (Some(4), Some(2)));
    let dest = parent.path().join("log.adi");
    let res = export::run_export(&lb.conn, &info, &r, &dest).unwrap();
    assert_eq!(res.exported, 4);
    let reasons: Vec<&str> = res.skipped.iter().map(|s| s.reason.as_str()).collect();
    assert!(reasons.contains(&"voided entry"));
    assert!(reasons.iter().any(|r| r.contains("no QSO fields")));
    assert!(res.warnings.iter().any(|w| w.contains("WEIRDMODE")));
    assert!(res.warnings.iter().any(|w| w.contains("non-ASCII")));
    let text = std::fs::read_to_string(&dest).unwrap();
    assert!(text.contains("<ADIF_VER:5>3.1.4"));
    let recs = parse_adi(&text);
    assert_eq!(recs.len(), 4);
    for rec in &recs {
        assert_eq!(rec["QSO_DATE"].len(), 8);
        assert!(rec["QSO_DATE"].chars().all(|c| c.is_ascii_digit()));
        assert_eq!(rec["TIME_ON"].len(), 6);
        assert!(rec.contains_key("BAND") || rec.contains_key("FREQ"));
        assert!(!rec["CALL"].is_empty());
    }
    let w1aw = recs.iter().find(|r| r["CALL"] == "W1AW").unwrap();
    assert_eq!((w1aw["BAND"].as_str(), w1aw["FREQ"].as_str(), w1aw["MODE"].as_str()), ("20m", "14.074", "FT8"));
    assert_eq!((w1aw["QSO_DATE"].as_str(), w1aw["TIME_ON"].as_str()), ("20261102", "190000"));
    let lsb = recs.iter().find(|r| r["CALL"] == "K1ABC/P").unwrap();
    assert_eq!((lsb["MODE"].as_str(), lsb["SUBMODE"].as_str(), lsb["BAND"].as_str()), ("SSB", "LSB", "40m"));
    let ve3 = recs.iter().find(|r| r["CALL"] == "VE3XYZ").unwrap();
    assert!(!ve3.contains_key("FREQ"), "never fabricate a frequency");
    assert_eq!(ve3["COMMENT"], "Good signal second line");
    assert_eq!(ve3["TIME_OFF"], "191230");
    let ja = recs.iter().find(|r| r["CALL"] == "JA1AAA").unwrap();
    assert!(!ja.contains_key("MODE") && !ja.contains_key("COMMENT"));
    // ADIF is refused for non-HAM books.
    let parent2 = tempfile::tempdir().unwrap();
    let (_d, g) = general_fixture(parent2.path());
    assert!(export::preview(&g.conn, LogbookType::General, &req(ExportFormat::Adif, "all")).is_err());
    assert!(adif::eligibility(&g.list_entries(&EntryQuery::default()).unwrap().items[0]).is_err());
}

#[test]
fn export_failure_leaves_no_partial_output() {
    let parent = tempfile::tempdir().unwrap();
    let (_dir, lb) = general_fixture(parent.path());
    let info = lb.info().unwrap();
    let dest = parent.path().join("never.pdf");
    emcomm_core::faults::inject("export.before_publish", emcomm_core::faults::Fault::DiskFull, 1);
    assert!(export::run_export(&lb.conn, &info, &req(ExportFormat::Pdf, "all"), &dest).is_err());
    emcomm_core::faults::clear();
    assert!(!dest.exists());
    let leftovers: Vec<_> =
        std::fs::read_dir(parent.path()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains(".tmp-")).collect();
    assert!(leftovers.is_empty());
}

#[test]
fn emergency_exports_do_not_claim_official_forms() {
    let parent = tempfile::tempdir().unwrap();
    let dir = create_book(parent.path(), LogbookType::Emergency, "Emergency Export");
    let mut lb = Logbook::open(&dir).unwrap();
    let mut m = input("Supply request");
    m.kind = EntryKind::Message;
    m.emergency = Some(EmergencyFields { message_number: "12".into(), originator: "Shelter-1".into(), ..Default::default() });
    lb.save_new(&m, &rid(), &operator()).unwrap();
    let info = lb.info().unwrap();
    for f in [ExportFormat::Txt, ExportFormat::Csv, ExportFormat::Docx] {
        let dest = parent.path().join(format!("em.{}", f.extension()));
        export::run_export(&lb.conn, &info, &req(f, "all"), &dest).unwrap();
        let content =
            if f == ExportFormat::Docx { unzip_text(&dest, "word/document.xml") } else { std::fs::read_to_string(&dest).unwrap() };
        assert!(content.contains("Shelter-1") && content.contains("Message Number"));
        assert!(!content.to_uppercase().contains("ICS 213") && !content.to_uppercase().contains("ICS-213"));
    }
}

#[test]
fn write_fixture_exports_for_external_validation() {
    let Ok(out) = std::env::var("EMCOMM_FIXTURE_OUT") else { return };
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out).unwrap();
    let parent = tempfile::tempdir().unwrap();
    let (_d, lb) = general_fixture(parent.path());
    let info = lb.info().unwrap();
    for f in [ExportFormat::Csv, ExportFormat::Txt, ExportFormat::Xlsx, ExportFormat::Docx, ExportFormat::Pdf] {
        let mut r = req(f, "all");
        r.include_voided = true;
        r.display_zone = "local".into();
        export::run_export(&lb.conn, &info, &r, &out.join(format!("fixture.{}", f.extension()))).unwrap();
    }
    let (_h, ham) = ham_fixture(parent.path());
    let hinfo = ham.info().unwrap();
    export::run_export(&ham.conn, &hinfo, &req(ExportFormat::Adif, "all"), &out.join("fixture.adi")).unwrap();
    // Large report to exercise pagination and repeated headings.
    let dir = create_book(parent.path(), LogbookType::Emergency, "Paging Fixture");
    let mut big = Logbook::open(&dir).unwrap();
    for i in 0..300 {
        let mut e = input(&format!("Paging entry {i}"));
        e.kind = EntryKind::Message;
        if i % 50 == 0 {
            e.body = long_body();
        }
        big.save_new(&e, &rid(), &operator()).unwrap();
    }
    let binfo = big.info().unwrap();
    export::run_export(&big.conn, &binfo, &req(ExportFormat::Pdf, "all"), &out.join("paging.pdf")).unwrap();
    export::run_export(&big.conn, &binfo, &req(ExportFormat::Docx, "all"), &out.join("paging.docx")).unwrap();
}
