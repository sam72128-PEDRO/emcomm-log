//! V2: operational periods, schema upgrade, ICS 213 / ICS 214 exports (A16–A19, A23, A24).

mod common;

use common::*;
use emcomm_core::export::forms::{BuiltForm, Form213Request, Form214Request, build_213, build_214, export_213};
use emcomm_core::export::ics::{page_count, read_values};
use emcomm_core::logbook::{DB_FILE, Logbook, PRE_MIGRATION_DIR};
use emcomm_core::migrate::{self, LOGBOOK_MIGRATIONS};
use emcomm_core::models::*;
use emcomm_core::package::{export_package, import_package};
use emcomm_core::periods;
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};

fn period(name: &str, start: &str, end: &str) -> OperationalPeriodInput {
    OperationalPeriodInput { name: name.into(), start_utc: start.into(), end_utc: end.into(), notes: String::new() }
}

fn emergency_book(parent: &Path) -> PathBuf {
    create_book(parent, LogbookType::Emergency, "Storm Forms")
}

fn message(subject: &str, at: &str) -> EntryInput {
    EntryInput {
        kind: EntryKind::Message,
        event_at_utc: at.into(),
        subject: subject.into(),
        body: "Need 40 cots and 80 blankets at Pine Rd shelter by 18:00.".into(),
        emergency: Some(EmergencyFields {
            recipient: "Jane Doe".into(),
            recipient_position: "Shelter Manager".into(),
            originator: "José Núñez".into(),
            originator_position: "EOC Radio Operator".into(),
            message_number: "0042".into(),
            sent_at_utc: Some(at.into()),
            approved_by_name: "R. Smith".into(),
            approved_by_position: "Shelter Lead".into(),
            response: "Approved. Truck 12 dispatched.".into(),
            replied_by_name: "M. Chen".into(),
            replied_by_position: "Logistics Chief".into(),
            replied_at_utc: Some("2026-11-02T19:30:00Z".into()),
            ..Default::default()
        }),
        ..input(subject)
    }
}

fn activity(subject: &str, at: &str) -> EntryInput {
    EntryInput {
        kind: EntryKind::Activity,
        event_at_utc: at.into(),
        subject: subject.into(),
        emergency: Some(EmergencyFields { activity_start_utc: Some(at.into()), ..Default::default() }),
        ..input(subject)
    }
}

fn r213(ids: Vec<String>) -> Form213Request {
    Form213Request { entry_ids: ids, display_zone: "utc".into() }
}

#[test]
fn a16_periods_create_edit_void_overlap_and_current() {
    let t = tempfile::tempdir().unwrap();
    let dir = emergency_book(t.path());
    let mut lb = Logbook::open(&dir).unwrap();
    let op1 = lb.save_period(None, &period("OP 1", "2026-11-02T12:00:00Z", "2026-11-02T20:00:00Z")).unwrap();
    assert!(op1.overlaps.is_empty());
    let op2 = lb.save_period(None, &period("OP 2", "2026-11-02T19:00:00-05:00", "2026-11-03T08:00:00Z")).unwrap();
    assert_eq!(op2.period.start_utc, "2026-11-03T00:00:00.000Z", "stored as UTC");
    assert!(op2.overlaps.is_empty());
    let handover = lb.save_period(None, &period("Handover", "2026-11-02T19:30:00Z", "2026-11-02T20:30:00Z")).unwrap();
    assert_eq!(handover.overlaps, vec!["OP 1".to_string()], "overlap is allowed but reported");

    let e = lb.save_period(None, &period("Bad", "2026-11-02T12:00:00Z", "2026-11-02T12:00:00Z")).unwrap_err();
    assert_eq!(e.field.as_deref(), Some("end_utc"));
    assert_eq!(
        lb.save_period(None, &period(" ", "2026-11-02T12:00:00Z", "2026-11-02T13:00:00Z")).unwrap_err().field.as_deref(),
        Some("name")
    );

    let edited = lb.save_period(Some(&op1.period.id), &period("OP 1 (day)", "2026-11-02T12:00:00Z", "2026-11-02T19:00:00Z")).unwrap();
    assert_eq!(edited.period.name, "OP 1 (day)");

    assert_eq!(periods::current_period(&lb.conn, "2026-11-02T15:00:00.000Z").unwrap().unwrap().name, "OP 1 (day)");
    assert_eq!(periods::current_period(&lb.conn, "2026-11-02T20:10:00.000Z").unwrap().unwrap().name, "Handover");
    assert!(periods::current_period(&lb.conn, "2026-11-02T22:00:00.000Z").unwrap().is_none(), "none between periods");
    assert!(periods::current_period(&lb.conn, "2026-11-01T00:00:00.000Z").unwrap().is_none());

    // Entries record their period; a voided period cannot take new entries but keeps old ones.
    let mut i = input("Net opened");
    i.op_id = Some(op1.period.id.clone());
    let saved = lb.save_new(&i, &rid(), &operator()).unwrap().entry;
    assert_eq!(saved.op_id.as_deref(), Some(op1.period.id.as_str()));
    lb.set_period_void(&op1.period.id, true).unwrap();
    assert!(lb.save_period(Some(&op1.period.id), &period("x", "2026-11-02T12:00:00Z", "2026-11-02T13:00:00Z")).is_err());
    let mut j = input("Too late");
    j.op_id = Some(op1.period.id.clone());
    assert_eq!(lb.save_new(&j, &rid(), &operator()).unwrap_err().field.as_deref(), Some("op_id"));
    let mut keep = i.clone();
    keep.subject = "Net opened (edited)".into();
    lb.save_changes(&saved.id, 1, &keep, "", &rid(), &operator()).expect("keeping a voided period is allowed");
    lb.set_period_void(&op1.period.id, false).unwrap();

    lb.set_period_resources(
        &op2.period.id,
        &[
            OpResource { name: "Pat Tester".into(), ics_position: "RADO".into(), home_agency: "MARC-5".into() },
            OpResource::default(),
            OpResource { name: "Lee Ortiz".into(), ics_position: "COML".into(), home_agency: "County EM".into() },
        ],
    )
    .unwrap();
    drop(lb);

    let lb = Logbook::open(&dir).unwrap();
    assert_eq!(lb.periods().unwrap().len(), 3);
    let res = lb.period_resources(&op2.period.id).unwrap();
    assert_eq!(res.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), vec!["Pat Tester", "Lee Ortiz"], "blank row dropped, order kept");
    assert_eq!(lb.get_entry(&saved.id).unwrap().op_id.as_deref(), Some(op1.period.id.as_str()));
}

/// Turn a freshly created logbook into a V1 (schema 1) logbook, as an older app version left it.
fn downgrade_to_v1(dir: &Path) {
    let mut c = Connection::open(dir.join(DB_FILE)).unwrap();
    migrate::rollback(&mut c, LOGBOOK_MIGRATIONS, 1).unwrap();
    c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).unwrap();
}

#[test]
fn a17_v1_logbook_and_package_upgrade_with_a_kept_copy() {
    let t = tempfile::tempdir().unwrap();
    let dir = emergency_book(t.path());
    {
        let mut lb = Logbook::open(&dir).unwrap();
        lb.save_new(&message("Before upgrade", "2026-11-02T18:00:00Z"), &rid(), &operator()).unwrap();
    }
    downgrade_to_v1(&dir);
    // A V1 package made from the V1 logbook.
    let pkg = t.path().join("v1.emlog");
    export_package(&dir, &pkg).unwrap();

    let lb = Logbook::open(&dir).unwrap();
    assert!(lb.notices.iter().any(|n| n.contains("upgraded from schema 1 to 2")), "{:?}", lb.notices);
    let copies: Vec<_> = fs::read_dir(dir.join(PRE_MIGRATION_DIR)).unwrap().flatten().collect();
    assert_eq!(copies.len(), 1);
    let old = Connection::open(copies[0].path()).unwrap();
    assert_eq!(migrate::current_version(&old).unwrap(), 1, "the kept copy is the untouched V1 database");
    let info = lb.info().unwrap();
    assert_eq!(info.schema_version, 2);
    assert_eq!(info.incident.operational_period, "OP03 1200-2000", "free-text period preserved, not converted");
    assert!(lb.periods().unwrap().is_empty(), "nothing is guessed from the free text");
    let page = lb.list_entries(&all_query()).unwrap();
    assert_eq!(page.items[0].subject, "Before upgrade");
    drop(lb);
    assert!(Logbook::open(&dir).unwrap().notices.iter().all(|n| !n.contains("upgraded")), "upgrade happens once");

    let other = tempfile::tempdir().unwrap();
    let imp = import_package(&pkg, other.path(), Some("Imported V1"), &[]).unwrap();
    let lb2 = Logbook::open(Path::new(&imp.path)).unwrap();
    assert_eq!(lb2.info().unwrap().schema_version, 2);
    assert_eq!(lb2.list_entries(&all_query()).unwrap().items.len(), 1);
}

#[test]
fn a18_ics_213_fills_every_block_and_continues_long_text() {
    let t = tempfile::tempdir().unwrap();
    let dir = emergency_book(t.path());
    let mut lb = Logbook::open(&dir).unwrap();
    let a = lb.save_new(&message("Shelter supply request ✓", "2026-11-02T18:05:00Z"), &rid(), &operator()).unwrap().entry;
    let mut long = message("Long situation report", "2026-11-02T18:10:00Z");
    long.body = (1..=1500).map(|i| format!("word{i}")).collect::<Vec<_>>().join(" ");
    let b = lb.save_new(&long, &rid(), &operator()).unwrap().entry;
    let traffic = lb.save_new(&input("Radio check"), &rid(), &operator()).unwrap().entry;
    let voided = lb.save_new(&message("Cancelled", "2026-11-02T18:20:00Z"), &rid(), &operator()).unwrap().entry;
    lb.void_entry(&voided.id, "sent in error", &rid(), &operator()).unwrap();
    let info = lb.info().unwrap();

    // Selected out of order: forms come out in sequence order; others are skipped with reasons.
    let req = r213(vec![b.id.clone(), traffic.id.clone(), a.id.clone(), voided.id.clone()]);
    let BuiltForm { pdf, exported, pages, skipped, warnings } = build_213(&lb.conn, &info, &req).unwrap();
    assert_eq!(exported, 2);
    assert_eq!(skipped.iter().map(|s| s.sequence_no).collect::<Vec<_>>(), vec![traffic.sequence_no, voided.sequence_no]);
    assert!(skipped.iter().any(|s| s.reason.contains("voided")));
    assert_eq!(pages, 4, "two forms + two continuation pages for the ~1,000 words that did not fit");
    assert_eq!(page_count(&pdf).unwrap(), 4);
    assert!(warnings.iter().any(|w| w.contains(&format!("#{}", b.sequence_no))));

    let v = read_values(&pdf).unwrap();
    assert_eq!(v["1 Incident Name Optional"], "Lake County Storm Response");
    assert_eq!(v["2 To Name and Position"], "Jane Doe, Shelter Manager");
    assert_eq!(v["3 From Name and Position"], "José Núñez, EOC Radio Operator");
    assert_eq!(v["4 Subject"], "Shelter supply request ✓");
    assert_eq!(v["5 Date"], "2026-11-02");
    assert_eq!(v["6 Time"], "18:05Z");
    assert_eq!(v["7 Message"], "Need 40 cots and 80 blankets at Pine Rd shelter by 18:00.");
    assert_eq!(v["8 Approved by Name"], "R. Smith");
    assert_eq!(v["PositionTitle_13"], "Shelter Lead");
    assert_eq!(v["9 Reply"], "Approved. Truck 12 dispatched.");
    assert_eq!(v["10 Replied by Name"], "M. Chen");
    assert_eq!(v["PositionTitle_14"], "Logistics Chief");
    assert_eq!(v["DateTime_14"], "2026-11-02 19:30Z");
    assert_eq!(v["4 Subject (2)"], "Long situation report");
    let shown = &v["7 Message (2)"];
    assert!(shown.starts_with("word1 ") && !shown.contains("word1500"), "long message continues elsewhere");
    // A23: no signature is ever filled.
    assert!(v.keys().all(|k| !k.starts_with("Signature")));

    // Nothing usable selected.
    assert!(build_213(&lb.conn, &info, &r213(vec![traffic.id.clone()])).is_err());
    // Written atomically to disk.
    let dest = t.path().join("213.pdf");
    let r = export_213(&lb.conn, &info, &r213(vec![a.id.clone()]), &dest).unwrap();
    assert_eq!(r.bytes, fs::metadata(&dest).unwrap().len());
    assert!(fs::read_dir(t.path()).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().contains(".tmp-")));
}

#[test]
fn a19_ics_214_selects_period_entries_in_order_and_continues_pages() {
    let t = tempfile::tempdir().unwrap();
    let dir = emergency_book(t.path());
    let mut lb = Logbook::open(&dir).unwrap();
    let op = lb.save_period(None, &period("OP 3", "2026-11-02T12:00:00Z", "2026-11-02T20:00:00Z")).unwrap().period;
    let other = lb.save_period(None, &period("OP 4", "2026-11-02T20:00:00Z", "2026-11-03T04:00:00Z")).unwrap().period;
    let late = lb.save_new(&activity("Second, by time", "2026-11-02T15:00:00Z"), &rid(), &operator()).unwrap().entry;
    let early = lb.save_new(&activity("First, by time", "2026-11-02T13:00:00Z"), &rid(), &operator()).unwrap().entry;
    lb.save_new(&activity("Outside the period", "2026-11-02T21:00:00Z"), &rid(), &operator()).unwrap();
    let mut assigned = activity("Assigned here though logged later", "2026-11-02T21:30:00Z");
    assigned.op_id = Some(op.id.clone());
    lb.save_new(&assigned, &rid(), &operator()).unwrap();
    let mut elsewhere = activity("Inside by time but assigned to OP 4", "2026-11-02T14:00:00Z");
    elsewhere.op_id = Some(other.id.clone());
    lb.save_new(&elsewhere, &rid(), &operator()).unwrap();
    let gone = lb.save_new(&activity("Voided", "2026-11-02T14:30:00Z"), &rid(), &operator()).unwrap().entry;
    lb.void_entry(&gone.id, "dup", &rid(), &operator()).unwrap();
    lb.save_new(&message("A message in the period", "2026-11-02T16:00:00Z"), &rid(), &operator()).unwrap();
    let resources: Vec<OpResource> =
        (1..=10).map(|i| OpResource { name: format!("Person {i}"), ics_position: "RADO".into(), home_agency: "MARC-5".into() }).collect();
    lb.set_period_resources(&op.id, &resources).unwrap();
    let info = lb.info().unwrap();

    let req = Form214Request {
        op_id: op.id.clone(),
        name: "Pat Tester".into(),
        ics_position: "Radio Operator".into(),
        home_agency: "MARC-5".into(),
        prepared_by_name: "Pat Tester".into(),
        prepared_by_position: "RADO".into(),
        display_zone: "utc".into(),
        ..Default::default()
    };
    let BuiltForm { pdf, exported, pages, warnings, .. } = build_214(&lb.conn, &info, &req).unwrap();
    assert_eq!(exported, 3, "activities in the period or assigned to it; not voided, not other periods, not messages");
    assert_eq!(pages, 2, "form page + resources continuation");
    assert!(warnings.iter().any(|w| w.contains("2 resources")));
    let v = read_values(&pdf).unwrap();
    assert_eq!(v["1 Incident Name_19"], "Lake County Storm Response");
    assert_eq!((v["Date From"].as_str(), v["Time From"].as_str()), ("2026-11-02", "12:00Z"));
    assert_eq!((v["Date To"].as_str(), v["Time To"].as_str()), ("2026-11-02", "20:00Z"));
    assert_eq!(v["3 Name"], "Pat Tester");
    assert_eq!(v["NameRow1_3"], "Person 1");
    assert_eq!(v["NameRow8"], "Person 8");
    assert_eq!(v["DateTimeRow1"], "2026-11-02 13:00Z");
    assert!(v["Notable ActivitiesRow1"].starts_with(&early.subject));
    assert!(v["Notable ActivitiesRow2"].starts_with(&late.subject));
    assert!(v["Notable ActivitiesRow3"].starts_with("Assigned here"));
    assert!(!v.contains_key("DateTimeRow4"));
    assert!(v.keys().all(|k| !k.starts_with("Signature")));

    // Messages can be included; details make rows longer.
    let with_msgs = Form214Request { kinds: vec![EntryKind::Activity, EntryKind::Message], ..req.clone() };
    let BuiltForm { pdf: pdf2, exported: n2, .. } = build_214(&lb.conn, &info, &with_msgs).unwrap();
    assert_eq!(n2, 4);
    assert!(read_values(&pdf2).unwrap().values().any(|s| s.starts_with("[Message] A message in the period")));

    // 70 entries: 24 rows on page 1, 36 on page 2, 10 on page 3 (header repeated), + resources page.
    for i in 0..67 {
        let at = format!("2026-11-02T16:{:02}:{:02}Z", i / 60, i % 60);
        lb.save_new(&activity(&format!("Check-in {i:02}"), &at), &rid(), &operator()).unwrap();
    }
    let BuiltForm { pdf: pdf3, exported: n3, pages: pages3, .. } = build_214(&lb.conn, &info, &req).unwrap();
    assert_eq!(n3, 70);
    assert_eq!(pages3, 4);
    let v3 = read_values(&pdf3).unwrap();
    assert!(v3.contains_key("Notable ActivitiesRow24"));
    assert!(v3.contains_key("Notable ActivitiesRow1_2 (2)"));
    assert!(v3.contains_key("Notable ActivitiesRow36 (2)"));
    assert!(v3.contains_key("Notable ActivitiesRow10_2 (3)"));
    assert!(!v3.contains_key("Notable ActivitiesRow11_2 (3)"));
    assert_eq!(v3["1 Incident Name_20 (3)"], "Lake County Storm Response", "header repeated on continuation pages");
    assert_eq!(v3["Date From (3)"], "2026-11-02");
}

#[test]
fn a24_approval_and_reply_fields_survive_revisions_restart_and_packages() {
    let t = tempfile::tempdir().unwrap();
    let dir = emergency_book(t.path());
    let id;
    {
        let mut lb = Logbook::open(&dir).unwrap();
        let e = lb.save_new(&message("Supply request", "2026-11-02T18:00:00Z"), &rid(), &operator()).unwrap().entry;
        let mut changed = message("Supply request", "2026-11-02T18:00:00Z");
        changed.emergency.as_mut().unwrap().approved_by_name = "A. Approver".into();
        lb.save_changes(&e.id, 1, &changed, "approver changed", &rid(), &operator()).unwrap();
        let revs = lb.revisions(&e.id).unwrap();
        assert_eq!(revs.len(), 2);
        assert!(revs[1].after.to_string().contains("A. Approver"));
        assert!(revs[1].before.as_ref().unwrap().to_string().contains("R. Smith"));
        id = e.id;
    }
    let lb = Logbook::open(&dir).unwrap();
    let m = lb.get_entry(&id).unwrap().emergency.unwrap();
    assert_eq!((m.approved_by_name.as_str(), m.replied_by_position.as_str()), ("A. Approver", "Logistics Chief"));
    assert_eq!(m.replied_at_utc.as_deref(), Some("2026-11-02T19:30:00.000Z"));
    drop(lb);
    let pkg = t.path().join("p.emlog");
    export_package(&dir, &pkg).unwrap();
    let other = tempfile::tempdir().unwrap();
    let imp = import_package(&pkg, other.path(), None, &[]).unwrap();
    let lb2 = Logbook::open(Path::new(&imp.path)).unwrap();
    let e2 = &lb2.list_entries(&all_query()).unwrap().items[0];
    assert_eq!(e2.emergency.as_ref().unwrap().replied_by_name, "M. Chen");
}
