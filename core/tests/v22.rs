//! 2.2: genuine 2.1 (schema 4) logbooks written by the 2.1 code upgrade to schema 5 with nothing changed
//! (DATA-02/03/06, DESK D-09). Fixtures: `tests/fixtures/v21/` (see the README there).

mod common;

use common::*;
use emcomm_core::logbook::{DB_FILE, Logbook, PRE_MIGRATION_DIR};
use emcomm_core::migrate::{self, LOGBOOK_MIGRATIONS};
use emcomm_core::models::*;
use emcomm_core::package::import_package;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/v21");

/// Copy a fixture logbook folder so the committed original is never opened for writing.
fn copy_fixture(name_prefix: &str, to: &Path) -> PathBuf {
    let src = std::fs::read_dir(FIXTURES)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.is_dir() && p.file_name().unwrap().to_string_lossy().starts_with(name_prefix))
        .expect("fixture folder");
    let dest = to.join(src.file_name().unwrap());
    copy_dir(&src, &dest);
    dest
}

fn copy_dir(src: &Path, dest: &Path) {
    std::fs::create_dir_all(dest).unwrap();
    for e in std::fs::read_dir(src).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &dest.join(e.file_name()));
        } else {
            std::fs::copy(&p, dest.join(e.file_name())).unwrap();
        }
    }
}

fn sha(path: &Path) -> String {
    hex::encode(Sha256::digest(std::fs::read(path).unwrap()))
}

/// What a 2.1 logbook holds, read straight from SQLite before 2.2 touches it.
fn v21_snapshot(dir: &Path) -> Vec<(String, i64, String, String, String, String)> {
    let c = rusqlite::Connection::open_with_flags(dir.join(DB_FILE), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let v: i64 = c.query_row("SELECT schema_version FROM logbook", [], |r| r.get(0)).unwrap();
    assert_eq!(v, 4, "fixture is a genuine schema-4 (2.1) logbook");
    let mut st = c
        .prepare(
            "SELECT id, sequence_no, status, operator_snapshot_json, station_snapshot_json,
                    COALESCE(operator_location_json,'') || '|' || COALESCE(reported_location_json,'')
             FROM entries ORDER BY sequence_no",
        )
        .unwrap();
    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))).unwrap().collect::<Result<_, _>>().unwrap()
}

#[test]
fn data_genuine_v21_ham_logbook_upgrades_to_v5_with_nothing_changed() {
    let t = tempfile::tempdir().unwrap();
    let dir = copy_fixture("V21_HAM_Fixture", t.path());
    let before = v21_snapshot(&dir);
    assert_eq!(before.len(), 3);
    let att_files: Vec<PathBuf> = std::fs::read_dir(dir.join("attachments")).unwrap().flatten().map(|e| e.path()).collect();
    assert_eq!(att_files.len(), 1);
    let att_hash = sha(&att_files[0]);

    let lb = Logbook::open(&dir).unwrap();
    assert_eq!(lb.info().unwrap().schema_version, 5);
    assert_eq!(migrate::latest_version(LOGBOOK_MIGRATIONS), 5);
    // A verified copy of the 2.1 database was kept before upgrading.
    let copies: Vec<_> = std::fs::read_dir(dir.join(PRE_MIGRATION_DIR)).unwrap().flatten().collect();
    assert_eq!(copies.len(), 1);
    // Every entry, number, operator, station and location is byte-for-byte what 2.1 wrote.
    let after: Vec<_> = {
        let mut st = lb
            .conn
            .prepare(
                "SELECT id, sequence_no, status, operator_snapshot_json, station_snapshot_json,
                        COALESCE(operator_location_json,'') || '|' || COALESCE(reported_location_json,'')
                 FROM entries ORDER BY sequence_no",
            )
            .unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(before, after);
    // New 2.2 fields start empty.
    let (imp, fu): (i64, String) =
        lb.conn.query_row("SELECT MAX(important), MAX(follow_up_state) FROM entries", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((imp, fu.as_str()), (0, "none"));
    for t in ["entry_links", "station_directory", "adif_import_batches", "entry_import_source"] {
        let n: i64 = lb.conn.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "{t} starts empty");
    }
    // Attachment bytes unchanged; caption empty, source 'file'.
    assert_eq!(sha(&att_files[0]), att_hash);
    let (cap, src): (String, String) =
        lb.conn.query_row("SELECT caption, source FROM attachments", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((cap.as_str(), src.as_str()), ("", "file"));
    // Station, shifts and numbering continue.
    assert_eq!(lb.station().unwrap().station_callsign, "W4CLB");
    let shifts = lb.operator_sessions().unwrap();
    assert_eq!(shifts.len(), 2);
    assert_eq!(lb.info().unwrap().next_sequence, 4);
    let voided = lb.list_entries(&EntryQuery { status: "void".into(), ..all_query() }).unwrap();
    assert_eq!(voided.items.len(), 1, "voided entry stays voided");
}

#[test]
fn data_genuine_v21_emergency_logbook_and_package_upgrade() {
    let t = tempfile::tempdir().unwrap();
    let dir = copy_fixture("V21_EMCOMM_Fixture", t.path());
    let before = v21_snapshot(&dir);
    let lb = Logbook::open(&dir).unwrap();
    assert_eq!(lb.info().unwrap().schema_version, 5);
    let items = lb.list_entries(&all_query()).unwrap().items;
    assert_eq!(items.len(), before.len());
    assert_eq!(items.iter().filter(|e| e.flagged).count(), 1, "Flag for Report kept");
    assert_eq!(items.iter().filter(|e| e.favorite).count(), 1, "Favorite kept");
    let periods: i64 = lb.conn.query_row("SELECT COUNT(*) FROM operational_periods", [], |r| r.get(0)).unwrap();
    assert_eq!(periods, 1, "operational period kept");
    let mut after_ids: Vec<(String, i64)> = items.iter().map(|e| (e.id.clone(), e.sequence_no)).collect();
    after_ids.sort_by_key(|x| x.1);
    let before_ids: Vec<(String, i64)> = before.iter().map(|b| (b.0.clone(), b.1)).collect();
    assert_eq!(after_ids, before_ids, "same IDs and numbers");
    drop(lb);

    // A 2.1 .emlog package imports into 2.2 and upgrades on the imported copy.
    let other = tempfile::tempdir().unwrap();
    let pkg = Path::new(FIXTURES).join("v21_ham.emlog");
    let imp = import_package(&pkg, other.path(), None, &[]).unwrap();
    let lb = Logbook::open(Path::new(&imp.path)).unwrap();
    assert_eq!(lb.info().unwrap().schema_version, 5);
    assert_eq!(lb.list_entries(&EntryQuery { status: "all".into(), ..all_query() }).unwrap().items.len(), 3);
}

/// Turn a saved entry back into an input for Save Changes (what the editor does).
fn as_input(e: &Entry) -> EntryInput {
    EntryInput {
        kind: e.kind,
        event_at_utc: e.event_at_utc.clone(),
        event_timezone: e.event_timezone.clone(),
        event_utc_offset_min: e.event_utc_offset_min,
        direction: e.direction,
        priority: e.priority,
        handling: e.handling,
        flagged: e.flagged,
        from_text: e.from_text.clone(),
        to_text: e.to_text.clone(),
        subject: e.subject.clone(),
        body: e.body.clone(),
        channel: e.channel.clone(),
        location: e.location.clone(),
        tags: e.tags.clone(),
        ham: e.ham.clone(),
        emergency: e.emergency.clone(),
        op_id: e.op_id.clone(),
        operator_location: e.operator_location.clone(),
        reported_location: e.reported_location.clone(),
        important: e.important,
        follow_up: e.follow_up.clone(),
        ..Default::default()
    }
}

#[test]
fn flag_important_and_follow_up_lifecycle_is_independent_and_recorded() {
    let t = tempfile::tempdir().unwrap();
    let dir = create_book(t.path(), LogbookType::Emergency, "Flags");
    let mut lb = Logbook::open(&dir).unwrap();
    let mut m = input("Shelter needs cots");
    m.kind = EntryKind::Message;
    m.handling = Some(Handling::AwaitingAck);
    m.important = true;
    m.follow_up = FollowUp {
        state: FollowUpState::Open,
        due_utc: Some("2026-11-02T22:00:00Z".into()),
        note: "Confirm delivery with Red Cross".into(),
        resolved_utc: Some("2000-01-01T00:00:00Z".into()), // ignored: the core owns it
    };
    let a = lb.save_new(&m, &rid(), &operator()).unwrap().entry;
    assert!(a.important && !a.flagged, "Important is not Flag for Report");
    assert_eq!((a.follow_up.state, a.follow_up.resolved_utc.as_deref()), (FollowUpState::Open, None));
    assert_eq!(a.follow_up.due_utc.as_deref(), Some("2026-11-02T22:00:00.000Z"));
    lb.save_new(&input("Routine check-in"), &rid(), &operator()).unwrap();
    let v = lb
        .save_new(
            &EntryInput { follow_up: FollowUp { state: FollowUpState::Open, ..Default::default() }, ..input("Voided") },
            &rid(),
            &operator(),
        )
        .unwrap();
    lb.void_entry(&v.entry.id, "duplicate", &rid(), &operator()).unwrap();

    let q = |fu: &str, imp: bool| EntryQuery { follow_up: fu.into(), important_only: imp, status: "active".into(), ..all_query() };
    assert_eq!(lb.list_entries(&q("open", false)).unwrap().total, 1, "voided entries are not open follow-ups");
    assert_eq!(lb.list_entries(&q("", true)).unwrap().total, 1);

    // Resolve: a recorded change; message handling and Flag for Report untouched.
    let mut i = as_input(&a);
    i.follow_up.state = FollowUpState::Resolved;
    let r = lb.save_changes(&a.id, a.current_revision, &i, "", &rid(), &operator()).unwrap().entry;
    assert_eq!(r.follow_up.state, FollowUpState::Resolved);
    let resolved_at = r.follow_up.resolved_utc.clone().expect("resolved time set");
    assert_eq!(r.handling, Some(Handling::AwaitingAck), "resolving is not an acknowledgement");
    assert!(!r.flagged);
    assert_eq!(lb.list_entries(&q("open", false)).unwrap().total, 0);
    assert_eq!(lb.list_entries(&q("resolved", false)).unwrap().total, 1);
    // Editing something else keeps the original resolved time.
    let mut i2 = as_input(&r);
    i2.follow_up.note = "Delivered 21:40".into();
    let r2 = lb.save_changes(&r.id, r.current_revision, &i2, "", &rid(), &operator()).unwrap().entry;
    assert_eq!(r2.follow_up.resolved_utc.as_deref(), Some(resolved_at.as_str()));
    // Reopen: resolved time cleared; history keeps every state.
    let mut i3 = as_input(&r2);
    i3.follow_up.state = FollowUpState::Open;
    let r3 = lb.save_changes(&r2.id, r2.current_revision, &i3, "", &rid(), &operator()).unwrap().entry;
    assert_eq!((r3.follow_up.state, r3.follow_up.resolved_utc.as_deref()), (FollowUpState::Open, None));
    let states: Vec<String> =
        lb.revisions(&a.id).unwrap().iter().map(|rv| rv.after["follow_up"]["state"].as_str().unwrap().to_string()).collect();
    assert_eq!(states, vec!["open", "resolved", "resolved", "open"]);
    // Clearing the follow-up drops its due time and note.
    let mut i4 = as_input(&r3);
    i4.follow_up.state = FollowUpState::None;
    let r4 = lb.save_changes(&r3.id, r3.current_revision, &i4, "", &rid(), &operator()).unwrap().entry;
    assert_eq!(r4.follow_up, FollowUp::default());
    // Bad due time is a validation error, not a silent drop.
    let mut bad = as_input(&r4);
    bad.follow_up = FollowUp { state: FollowUpState::Open, due_utc: Some("tomorrow".into()), ..Default::default() };
    assert_eq!(
        lb.save_changes(&r4.id, r4.current_revision, &bad, "", &rid(), &operator()).unwrap_err().field.as_deref(),
        Some("follow_up.due_utc")
    );
}

#[test]
fn dir_per_logbook_directory_snapshots_and_travels() {
    use emcomm_core::directory::{DirectoryInput, DirectoryKind};
    use emcomm_core::package::export_package;
    let t = tempfile::tempdir().unwrap();
    let dir = create_book(t.path(), LogbookType::Emergency, "Directory");
    let mut lb = Logbook::open(&dir).unwrap();
    let shelter = lb
        .save_directory(
            None,
            &DirectoryInput {
                label: "Shelter 2".into(),
                kind: DirectoryKind::TacticalIdentifier,
                identifier: " shelter  2 ".into(),
                agency: "Red Cross".into(),
                channel: "TAC 2".into(),
                aliases: vec!["Lake HS".into(), "lake hs".into()],
                tags: vec!["shelter".into()],
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(shelter.entry.aliases, vec!["Lake HS"], "duplicate alias dropped");
    assert_eq!(shelter.entry.tags, vec!["SHELTER"]);
    let ham = lb
        .save_directory(
            None,
            &DirectoryInput {
                label: "Net control".into(),
                kind: DirectoryKind::AmateurCallsign,
                identifier: "w4clb".into(),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(ham.entry.identifier, "W4CLB", "callsigns normalized");
    assert!(lb.save_directory(None, &DirectoryInput { label: " ".into(), ..Default::default() }).is_err(), "label required");
    // Same identifier with another meaning is allowed, with a warning.
    let twin = lb
        .save_directory(
            None,
            &DirectoryInput { label: "Shelter 2 (old site)".into(), identifier: "SHELTER 2".into(), ..Default::default() },
        )
        .unwrap();
    assert_eq!(twin.similar.len(), 1);
    assert_eq!(lb.list_directory("lake").unwrap().len(), 1, "alias search");
    assert_eq!(lb.list_directory("").unwrap().len(), 3);

    // An entry made from the directory keeps its own text; editing the directory never rewrites it.
    let e = lb
        .save_new(&EntryInput { from_text: "Shelter 2".into(), channel: "TAC 2".into(), ..input("Cots needed") }, &rid(), &operator())
        .unwrap()
        .entry;
    lb.mark_directory_used(&shelter.entry.id, &e.id).unwrap();
    assert_eq!(lb.list_directory("").unwrap()[0].id, shelter.entry.id, "recently used ranks first");
    lb.save_directory(
        Some(&shelter.entry.id),
        &DirectoryInput { label: "Shelter 2".into(), channel: "TAC 5".into(), ..Default::default() },
    )
    .unwrap();
    let again = lb.get_entry(&e.id).unwrap();
    assert_eq!((again.from_text.as_str(), again.channel.as_str()), ("Shelter 2", "TAC 2"));

    // Another logbook has its own, empty directory.
    let other = create_book(t.path(), LogbookType::Emergency, "Other");
    assert!(Logbook::open(&other).unwrap().list_directory("").unwrap().is_empty());

    // The directory travels inside the .emlog package.
    lb.delete_directory(&twin.entry.id).unwrap();
    drop(lb);
    let pkg = t.path().join("dir.emlog");
    export_package(&dir, &pkg).unwrap();
    let dest = tempfile::tempdir().unwrap();
    let imp = import_package(&pkg, dest.path(), None, &[]).unwrap();
    let lb2 = Logbook::open(Path::new(&imp.path)).unwrap();
    let items = lb2.list_directory("").unwrap();
    assert_eq!(items.len(), 2);
    assert!(items.iter().any(|d| d.identifier == "W4CLB" && d.kind == DirectoryKind::AmateurCallsign));
}
