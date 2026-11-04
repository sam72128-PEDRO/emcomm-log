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

#[test]
fn rel_links_are_one_hop_audited_and_never_change_entries() {
    use emcomm_core::links::Relation::*;
    use emcomm_core::package::export_package;
    let t = tempfile::tempdir().unwrap();
    let dir = create_book(t.path(), LogbookType::Emergency, "Links");
    let mut lb = Logbook::open(&dir).unwrap();
    let ids: Vec<Entry> = (1..=11).map(|i| lb.save_new(&input(&format!("Msg {i}")), &rid(), &operator()).unwrap().entry).collect();
    let (e8, e10, e11) = (&ids[7], &ids[9], &ids[10]);
    let rev_before: Vec<i64> = ids.iter().map(|e| e.current_revision).collect();

    let a = lb.add_link(&e10.id, &e8.id, RelayOf, "via MARC-5", &operator()).unwrap();
    assert!(a.outgoing && a.counterpart.sequence_no == 8);
    lb.add_link(&e11.id, &e10.id, ResponseTo, "", &operator()).unwrap();
    // Rejections leave the database as it was.
    assert!(lb.add_link(&e8.id, &e8.id, RelatedTo, "", &operator()).is_err(), "self-link");
    assert!(lb.add_link(&e10.id, &e8.id, RelayOf, "", &operator()).is_err(), "duplicate");
    assert!(lb.add_link(&e8.id, &e10.id, RelayOf, "", &operator()).is_err(), "two-way loop");
    lb.add_link(&e8.id, &e11.id, RelatedTo, "", &operator()).unwrap();
    assert!(lb.add_link(&e11.id, &e8.id, RelatedTo, "", &operator()).is_err(), "symmetric stored once");
    assert!(lb.add_link(&e8.id, "no-such-entry", RelatedTo, "", &operator()).is_err());
    // A different relation between the same two entries is allowed (a multi-step chain).
    lb.add_link(&e10.id, &e8.id, FollowUpTo, "", &operator()).unwrap();

    // One hop from #10: relay of #8, follow-up to #8, answered by #11.
    let v10 = lb.entry_links(&e10.id, false).unwrap();
    assert_eq!(v10.len(), 3);
    assert!(v10.iter().any(|v| v.link.relation == ResponseTo && !v.outgoing && v.counterpart.sequence_no == 11));
    let page = lb.list_entries(&all_query()).unwrap();
    let count = |n: i64| page.items.iter().find(|e| e.sequence_no == n).unwrap().related_count;
    assert_eq!((count(8), count(10), count(11), count(1)), (3, 3, 2, 0));

    // Linking changed no entry: same revisions, same numbers, no new entries.
    let now: Vec<i64> = (1..=11).map(|i| page.items.iter().find(|e| e.sequence_no == i).unwrap().current_revision).collect();
    assert_eq!(now, rev_before);
    assert_eq!(page.total, 11);

    // Voiding #8 keeps the link, shown as voided.
    lb.void_entry(&e8.id, "wrong station", &rid(), &operator()).unwrap();
    let v10 = lb.entry_links(&e10.id, false).unwrap();
    assert!(v10.iter().filter(|v| v.counterpart.sequence_no == 8).all(|v| v.counterpart.status == EntryStatus::Void));

    // Removing a link is recorded on the link; it stays visible in the full history.
    lb.remove_link(&a.link.id, &operator()).unwrap();
    assert!(lb.remove_link(&a.link.id, &operator()).is_err(), "already removed");
    assert_eq!(lb.entry_links(&e10.id, false).unwrap().len(), 2);
    let all = lb.entry_links(&e10.id, true).unwrap();
    let removed = all.iter().find(|v| v.link.id == a.link.id).unwrap();
    assert!(removed.link.removed_at_utc.is_some() && removed.link.removed_by.is_some());
    // The database refuses to delete or rewrite a link.
    assert!(lb.conn.execute("DELETE FROM entry_links", []).is_err());
    assert!(lb.conn.execute("UPDATE entry_links SET relation='corrects'", []).is_err());

    // Links travel in the package with both ends present.
    drop(lb);
    let pkg = t.path().join("links.emlog");
    export_package(&dir, &pkg).unwrap();
    let dest = tempfile::tempdir().unwrap();
    let imp = import_package(&pkg, dest.path(), None, &[]).unwrap();
    let lb2 = Logbook::open(Path::new(&imp.path)).unwrap();
    assert_eq!(lb2.entry_links(&e10.id, true).unwrap().len(), 3);
}

#[test]
fn att_capture_caption_remove_and_travel_with_hashes() {
    use emcomm_core::attach::{is_risky, sniff_file};
    use emcomm_core::package::export_package;
    use std::sync::atomic::AtomicBool;
    let t = tempfile::tempdir().unwrap();
    let dir = create_book(t.path(), LogbookType::General, "Attachments");
    let mut lb = Logbook::open(&dir).unwrap();

    // A photo dropped onto the editor (named .txt on purpose: the type comes from the content).
    let photo = t.path().join("IMG_0001.txt");
    std::fs::write(&photo, b"\xFF\xD8\xFF\xE0 jpeg-ish bytes with EXIF GPS inside").unwrap();
    let mut progress = vec![];
    let mut a = lb.stage_attachment_from(&photo, "drag_drop", None, &mut |d, t| progress.push((d, t))).unwrap();
    assert_eq!((a.source.as_str(), a.mime.as_str()), ("drag_drop", "image/jpeg"));
    assert!(!progress.is_empty());
    std::fs::remove_file(&photo).unwrap(); // the source can vanish: the managed copy is what counts
    a.caption = "Bridge damage, north side".into();
    // A pasted screenshot.
    let shot = lb.stage_attachment_bytes("Screenshot 2026-11-04.png", b"\x89PNG\r\n\x1a\n...", "paste").unwrap();
    assert_eq!(shot.mime, "image/png");
    // A cancelled copy leaves nothing staged.
    let big = t.path().join("big.bin");
    std::fs::write(&big, vec![7u8; 3 * 1024 * 1024]).unwrap();
    let staged_before = std::fs::read_dir(dir.join(".staging")).unwrap().count();
    let cancel = AtomicBool::new(true);
    assert!(lb.stage_attachment_from(&big, "file", Some(&cancel), &mut |_, _| {}).is_err());
    assert_eq!(std::fs::read_dir(dir.join(".staging")).unwrap().count(), staged_before);
    assert!(lb.stage_attachment_from(&big, "carrier_pigeon", None, &mut |_, _| {}).is_err());

    let e = lb.save_new(&EntryInput { new_attachments: vec![a, shot], ..input("Bridge report") }, &rid(), &operator()).unwrap().entry;
    assert_eq!(e.attachments.len(), 2);
    let photo_att = e.attachments.iter().find(|x| x.source == "drag_drop").unwrap().clone();
    assert_eq!(photo_att.caption, "Bridge damage, north side");
    let shot_att = e.attachments.iter().find(|x| x.source == "paste").unwrap().clone();

    // Save As writes an identical copy.
    let out = t.path().join("export-copy.jpg");
    lb.save_attachment_as(&photo_att.id, &out).unwrap();
    assert_eq!(sha(&out), photo_att.sha256);
    // Programs are never opened, whatever their name.
    let prog = t.path().join("invoice.pdf");
    std::fs::write(&prog, b"MZ\x90\x00 windows program").unwrap();
    assert!(is_risky("invoice.pdf", sniff_file(&prog).unwrap()));

    // Caption change + removal are one recorded change; the removed file stays for history and backups.
    let mut i = as_input(&e);
    i.attachment_captions = vec![AttachmentCaption { id: photo_att.id.clone(), caption: "Bridge damage (north)".into() }];
    i.remove_attachments = vec![shot_att.id.clone()];
    let e2 = lb.save_changes(&e.id, e.current_revision, &i, "tidy attachments", &rid(), &operator()).unwrap().entry;
    assert_eq!(e2.current_revision, 2);
    assert_eq!(e2.attachments.len(), 1);
    assert_eq!(e2.attachments[0].caption, "Bridge damage (north)");
    let revs = lb.revisions(&e.id).unwrap();
    assert_eq!(revs[1].before.as_ref().unwrap()["attachments"].as_array().unwrap().len(), 2, "history keeps it");
    assert!(dir.join(&shot_att.rel_path).is_file(), "file kept");
    assert_eq!(lb.cleanup_orphans(&Default::default()).unwrap(), 0, "a removed attachment is not an orphan");
    // Removing something not on the entry fails without changing anything.
    let mut bad = as_input(&e2);
    bad.remove_attachments = vec![shot_att.id.clone()];
    assert!(lb.save_changes(&e2.id, e2.current_revision, &bad, "", &rid(), &operator()).is_err());

    // Package: captions, sources and every file travel; hashes intact.
    drop(lb);
    let pkg = t.path().join("att.emlog");
    export_package(&dir, &pkg).unwrap();
    let dest = tempfile::tempdir().unwrap();
    let imp = import_package(&pkg, dest.path(), None, &[]).unwrap();
    let lb2 = Logbook::open(Path::new(&imp.path)).unwrap();
    let got = lb2.get_entry(&e.id).unwrap();
    assert_eq!((got.attachments.len(), got.attachments[0].caption.as_str()), (1, "Bridge damage (north)"));
    assert_eq!(sha(&Path::new(&imp.path).join(&got.attachments[0].rel_path)), photo_att.sha256);
    assert!(Path::new(&imp.path).join(&shot_att.rel_path).is_file(), "removed attachment's file travels too");
}

fn adi_field(n: &str, v: &str) -> String {
    format!("<{n}:{}>{v}", v.len())
}
fn adi_rec(fields: &[(&str, &str)]) -> String {
    fields.iter().map(|(n, v)| adi_field(n, v)).collect::<String>() + "<EOR>\n"
}

#[test]
fn adif_import_preview_duplicates_atomic_and_idempotent() {
    use emcomm_core::adif_import::{ImportOptions, RowStatus};
    use emcomm_core::faults::{self, Fault};
    let t = tempfile::tempdir().unwrap();
    let dir = create_book(t.path(), LogbookType::Ham, "Import");
    let mut lb = Logbook::open(&dir).unwrap();
    // One QSO already in the log.
    lb.save_new(&qso("W1AW", "2026-11-02T19:00:00Z", Some(14_062_000), "20m", "CW"), &rid(), &operator()).unwrap();

    let file = format!(
        "QRZ export\n{}<EOH>\n{}{}{}{}{}{}{}{}",
        adi_field("ADIF_VER", "3.1.8") + &adi_field("PROGRAMID", "QRZLogbook"),
        // 1: same as the existing W1AW (call, minute, band, mode, station) -> duplicate
        adi_rec(&[
            ("CALL", "W1AW"),
            ("QSO_DATE", "20261102"),
            ("TIME_ON", "1900"),
            ("FREQ", "14.062"),
            ("MODE", "CW"),
            ("STATION_CALLSIGN", "KJ4MRC")
        ]),
        // 2+3: POTA activator, same call/time/band, two different parks -> two QSOs, both new
        adi_rec(&[
            ("CALL", "K1ABC"),
            ("QSO_DATE", "20261102"),
            ("TIME_ON", "2000"),
            ("BAND", "40M"),
            ("MODE", "SSB"),
            ("MY_SIG", "POTA"),
            ("MY_SIG_INFO", "US-0005"),
            ("SIG", "POTA"),
            ("SIG_INFO", "US-1234")
        ]),
        adi_rec(&[
            ("CALL", "K1ABC"),
            ("QSO_DATE", "20261102"),
            ("TIME_ON", "2000"),
            ("BAND", "40M"),
            ("MODE", "SSB"),
            ("MY_POTA_REF", "US-0006")
        ]),
        // 4: SOTA activator; 5: SOTA chaser with Unicode notes
        adi_rec(&[
            ("CALL", "G4XYZ"),
            ("QSO_DATE", "20261103"),
            ("TIME_ON", "000130"),
            ("FREQ", "10.118"),
            ("MODE", "CW"),
            ("MY_SOTA_REF", "W4C/CM-001"),
            ("OPERATOR", "KJ4AAA")
        ]),
        adi_rec(&[
            ("CALL", "DL1ABC"),
            ("QSO_DATE", "20261103"),
            ("TIME_ON", "0100"),
            ("FREQ", "7.030"),
            ("MODE", "CW"),
            ("SOTA_REF", "W7A/AP-001"),
            ("NOTES", "Grüße — 73")
        ]),
        // 6: invalid date; 7: no call
        adi_rec(&[("CALL", "N0BAD"), ("QSO_DATE", "2026-11-02"), ("TIME_ON", "1900")]),
        adi_rec(&[("QSO_DATE", "20261102"), ("TIME_ON", "1900")]),
        // 8: same contact as #4 again inside the file
        adi_rec(&[
            ("CALL", "G4XYZ"),
            ("QSO_DATE", "20261103"),
            ("TIME_ON", "000130"),
            ("FREQ", "10.118"),
            ("MODE", "CW"),
            ("MY_SOTA_REF", "W4C/CM-001"),
            ("OPERATOR", "KJ4AAA")
        ]),
    );
    let bytes = file.as_bytes();
    let opts = ImportOptions { default_station_call: "W4CLB".into(), default_operator_call: String::new() };
    let pv = lb.adif_preview(bytes, "qrz.adi", &opts).unwrap();
    assert_eq!(pv.program, "QRZLogbook");
    assert_eq!((pv.records, pv.ok, pv.duplicates, pv.invalid), (8, 4, 2, 2));
    let by = |rec: usize| pv.rows.iter().find(|r| r.record == rec).unwrap();
    assert_eq!(by(1).status, RowStatus::Duplicate);
    assert_eq!(by(1).duplicate_of, Some(1));
    assert_eq!((by(2).status.clone(), by(3).status.clone()), (RowStatus::Ok, RowStatus::Ok), "multi-park is not a duplicate");
    assert_eq!(by(2).station_call, "W4CLB", "default station callsign fills a missing one");
    assert_eq!(by(4).operator_call, "KJ4AAA");
    assert!(by(6).reason.contains("YYYYMMDD") && by(7).reason.contains("CALL"));
    assert!(by(8).reason.contains("earlier in this file"));
    // Nothing was written by the preview.
    assert_eq!(lb.list_entries(&all_query()).unwrap().total, 1);

    // Default selection = the OK rows. A failure before commit leaves nothing behind.
    let selected: Vec<String> = pv.rows.iter().filter(|r| r.status == RowStatus::Ok).map(|r| r.key.clone()).collect();
    faults::inject("adif.import.before_commit", Fault::Io, 1);
    assert!(lb.adif_import(bytes, "qrz.adi", &pv.source_sha256, &selected, &opts, &operator()).is_err());
    faults::clear();
    assert_eq!(lb.list_entries(&all_query()).unwrap().total, 1, "atomic: no partial import");
    let n: i64 = lb.conn.query_row("SELECT COUNT(*) FROM adif_import_batches", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
    // A different file than the one previewed is refused.
    assert!(lb.adif_import(b"<CALL:4>K1AB<EOR>", "x.adi", &pv.source_sha256, &selected, &opts, &operator()).is_err());

    let res = lb.adif_import(bytes, "qrz.adi", &pv.source_sha256, &selected, &opts, &operator()).unwrap();
    assert_eq!((res.imported, res.first_sequence, res.last_sequence), (4, Some(2), Some(5)), "next numbers in order");
    let items = lb.list_entries(&all_query()).unwrap().items;
    let g4 = items.iter().find(|e| e.ham.as_ref().is_some_and(|h| h.their_call == "G4XYZ")).unwrap();
    assert_eq!(g4.event_at_utc, "2026-11-03T00:01:30.000Z", "contact time in UTC, not import time");
    assert_eq!(g4.ham.as_ref().unwrap().my_sota_ref, "W4C/CM-001");
    assert_eq!(g4.operator_snapshot.name, operator().name, "who imported it");
    assert_eq!(g4.station_snapshot.station_callsign, "W4CLB", "the QSO's own station, not the logbook's current one");
    let dl = items.iter().find(|e| e.ham.as_ref().is_some_and(|h| h.their_call == "DL1ABC")).unwrap();
    assert_eq!(dl.ham.as_ref().unwrap().qso_notes, "Grüße — 73");

    // Importing the same file again doubles nothing: every row is now a duplicate, and the earlier batch shows.
    let again = lb.adif_preview(bytes, "qrz.adi", &opts).unwrap();
    assert_eq!(again.ok, 0);
    assert_eq!(again.previous_batches.len(), 1);
    assert!(
        again.rows.iter().filter(|r| r.status == RowStatus::Duplicate).all(|r| r.duplicate_of.is_some() || r.reason.contains("earlier"))
    );
    // An explicitly chosen duplicate can still be imported ("Import selected anyway").
    let pick = vec![again.rows.iter().find(|r| r.record == 5).unwrap().key.clone()];
    assert_eq!(lb.adif_import(bytes, "qrz.adi", &again.source_sha256, &pick, &opts, &operator()).unwrap().imported, 1);

    // Our own 2.1 ADIF export imports into another logbook as the same QSOs.
    let info = lb.info().unwrap();
    let out = t.path().join("export.adi");
    let mut r = emcomm_core::export::ExportRequest {
        format: emcomm_core::export::ExportFormat::Adif,
        scope: "all".into(),
        range_from: None,
        range_to: None,
        filter: None,
        include_voided: false,
        display_zone: "utc".into(),
        adif_preset: Default::default(),
        include_precise_coordinates: false,
        reviewed: None,
    };
    r.adif_preset = emcomm_core::export::adif::AdifPreset::General;
    let exp = emcomm_core::export::run_export(&lb.conn, &info, &r, &out).unwrap();
    let other = create_book(t.path(), LogbookType::Ham, "Round trip");
    let mut lb2 = Logbook::open(&other).unwrap();
    let ob = std::fs::read(&out).unwrap();
    let pv2 = lb2.adif_preview(&ob, "export.adi", &ImportOptions::default()).unwrap();
    assert_eq!(pv2.invalid, 0, "{:?}", pv2.rows.iter().filter(|r| r.status == RowStatus::Invalid).collect::<Vec<_>>());
    assert_eq!(pv2.ok as i64 + pv2.duplicates as i64, exp.exported);
    // A general logbook cannot import ADIF.
    let gen_dir = create_book(t.path(), LogbookType::General, "General");
    assert!(Logbook::open(&gen_dir).unwrap().adif_preview(&ob, "x.adi", &ImportOptions::default()).is_err());
    let keys: Vec<String> = pv2.rows.iter().filter(|r| r.status == RowStatus::Ok).map(|r| r.key.clone()).collect();
    lb2.adif_import(&ob, "export.adi", &pv2.source_sha256, &keys, &ImportOptions::default(), &operator()).unwrap();
}

#[test]
fn adif_import_1000_qsos_with_parks_and_duplicates() {
    use emcomm_core::adif_import::{ImportOptions, RowStatus};
    let t = tempfile::tempdir().unwrap();
    let dir = create_book(t.path(), LogbookType::Ham, "Big import");
    let mut lb = Logbook::open(&dir).unwrap();
    let mut file = String::from("<ADIF_VER:5>3.1.8<EOH>\n");
    for i in 0..1000 {
        let call = format!("K{}AB", i % 10);
        let time = format!("{:02}{:02}", (i / 60) % 24, i % 60);
        let park = if i % 2 == 0 { "US-0005" } else { "US-0006" };
        // Every 10th record repeats the previous one (a duplicate inside the file).
        let n = if i % 10 == 9 { i - 1 } else { i };
        let time = if i % 10 == 9 { format!("{:02}{:02}", (n / 60) % 24, n % 60) } else { time };
        let park = if i % 10 == 9 { if n % 2 == 0 { "US-0005" } else { "US-0006" } } else { park };
        let call = if i % 10 == 9 { format!("K{}AB", n % 10) } else { call };
        file += &adi_rec(&[
            ("CALL", &call),
            ("QSO_DATE", "20261104"),
            ("TIME_ON", &time),
            ("BAND", "20m"),
            ("MODE", "FT8"),
            ("MY_POTA_REF", park),
            ("STATION_CALLSIGN", "W4CLB"),
        ]);
    }
    let started = std::time::Instant::now();
    let pv = lb.adif_preview(file.as_bytes(), "big.adi", &ImportOptions::default()).unwrap();
    let preview_time = started.elapsed();
    assert_eq!((pv.ok, pv.duplicates, pv.invalid), (900, 100, 0));
    let keys: Vec<String> = pv.rows.iter().filter(|r| r.status == RowStatus::Ok).map(|r| r.key.clone()).collect();
    let started = std::time::Instant::now();
    let res = lb.adif_import(file.as_bytes(), "big.adi", &pv.source_sha256, &keys, &ImportOptions::default(), &operator()).unwrap();
    println!("1000-record ADIF: preview {preview_time:?}, import {:?}", started.elapsed());
    assert_eq!((res.imported, res.first_sequence, res.last_sequence), (900, Some(1), Some(900)));
}

#[test]
fn exp_review_exact_selection_snapshot_and_ics_eligibility() {
    use emcomm_core::export::forms::{self, Form213Request, Form214Request};
    use emcomm_core::export::review::ReviewRef;
    use emcomm_core::export::{self, ExportFormat, ExportRequest};
    let t = tempfile::tempdir().unwrap();
    let dir = create_book(t.path(), LogbookType::Emergency, "Review");
    let mut lb = Logbook::open(&dir).unwrap();
    let op = lb
        .save_period(
            None,
            &OperationalPeriodInput {
                name: "OP1".into(),
                start_utc: "2026-11-02T12:00:00Z".into(),
                end_utc: "2026-11-02T20:00:00Z".into(),
                notes: String::new(),
            },
        )
        .unwrap()
        .period;
    let mut all = Vec::new();
    for i in 0..12 {
        let mut e = input(&format!("Entry {i}"));
        e.kind = if i % 3 == 0 { EntryKind::Traffic } else { EntryKind::Message };
        e.event_at_utc = if i == 11 { "2026-11-02T22:00:00Z".into() } else { format!("2026-11-02T13:{:02}:00Z", i * 4) };
        all.push(lb.save_new(&e, &rid(), &operator()).unwrap().entry);
    }
    let info = lb.info().unwrap();
    let refs = |idx: &[usize]| -> Vec<ReviewRef> {
        idx.iter().map(|&i| ReviewRef { id: all[i].id.clone(), revision: all[i].current_revision }).collect()
    };
    let req = |f: ExportFormat, reviewed: Option<Vec<ReviewRef>>| ExportRequest {
        format: f,
        scope: "all".into(),
        range_from: None,
        range_to: None,
        filter: None,
        include_voided: false,
        display_zone: "utc".into(),
        adif_preset: Default::default(),
        include_precise_coordinates: false,
        reviewed,
    };
    // Review lists all 12, all eligible, and changes nothing.
    let rv = export::review(&lb.conn, info.logbook_type, &req(ExportFormat::Csv, None)).unwrap();
    assert_eq!((rv.rows.len(), rv.eligible), (12, 12));
    let revs: Vec<i64> = lb.list_entries(&all_query()).unwrap().items.iter().map(|e| e.current_revision).collect();
    assert!(revs.iter().all(|&r| r == 1), "reviewing is not an edit");
    // 5 of 12 -> exactly 5 in every generic format.
    let five = refs(&[1, 2, 4, 7, 10]);
    for f in [ExportFormat::Csv, ExportFormat::Txt, ExportFormat::Xlsx, ExportFormat::Docx, ExportFormat::Pdf] {
        let dest = t.path().join(format!("five.{}", f.extension()));
        let res = export::run_export(&lb.conn, &info, &req(f, Some(five.clone())), &dest).unwrap();
        assert_eq!(res.exported, 5, "{f:?}");
    }
    let csv = std::fs::read_to_string(t.path().join("five.csv")).unwrap();
    assert!(csv.contains("Entry 7") && !csv.contains("Entry 3"));
    // Void one between review and export: blocked, nothing written.
    lb.void_entry(&all[7].id, "wrong", &rid(), &operator()).unwrap();
    let dest = t.path().join("blocked.csv");
    let err = export::run_export(&lb.conn, &info, &req(ExportFormat::Csv, Some(five.clone())), &dest).unwrap_err();
    assert!(err.message.contains("#8") && err.message.contains("voided"), "{}", err.message);
    assert!(!dest.exists());
    // The voided entry now shows in the review as excluded, with the reason.
    let rv = export::review(&lb.conn, info.logbook_type, &req(ExportFormat::Csv, None)).unwrap();
    assert!(rv.rows.iter().any(|r| r.sequence_no == 8 && !r.eligible && r.reason.contains("Voided")));

    // ICS 214 from a Traffic Log selection: the out-of-period entry (#12) is blocked, never added.
    let sel = vec![all[0].id.clone(), all[3].id.clone(), all[11].id.clone()];
    let r214 = Form214Request {
        op_id: op.id.clone(),
        kinds: vec![EntryKind::Traffic, EntryKind::Message],
        entry_ids: sel.clone(),
        ..Default::default()
    };
    let rv = forms::review_214(&lb.conn, &info, &r214).unwrap();
    let blocked = rv.rows.iter().find(|r| r.sequence_no == 12).unwrap();
    assert!(!blocked.eligible && blocked.reason.contains("outside operational period"));
    // Forcing the blocked entry into the reviewed set is refused.
    let bad = Form214Request { reviewed: Some(refs(&[0, 11])), ..r214.clone() };
    assert!(forms::export_214(&lb.conn, &info, &bad, &t.path().join("bad214.pdf")).is_err());
    // Reviewed set {#1} (unchecked #4): exactly one row exported, #4 reported as left out.
    let ok = Form214Request { reviewed: Some(refs(&[0])), ..r214 };
    let res = forms::export_214(&lb.conn, &info, &ok, &t.path().join("ok214.pdf")).unwrap();
    assert_eq!(res.exported, 1);
    assert!(res.skipped.iter().any(|s| s.sequence_no == 4 && s.reason == "left out in review"));
    assert!(res.skipped.iter().any(|s| s.sequence_no == 12));

    // ICS 213: one form per message; the Activity entry is excluded with its reason.
    let r213 = Form213Request {
        entry_ids: vec![all[0].id.clone(), all[1].id.clone(), all[2].id.clone()],
        display_zone: "utc".into(),
        reviewed: None,
    };
    let rv = forms::review_213(&lb.conn, &info, &r213).unwrap();
    assert_eq!(rv.eligible, 2);
    assert!(rv.rows.iter().any(|r| r.sequence_no == 1 && r.reason.contains("not a Message")));
    let res =
        forms::export_213(&lb.conn, &info, &Form213Request { reviewed: Some(refs(&[2])), ..r213 }, &t.path().join("213.pdf")).unwrap();
    assert_eq!(res.exported, 1);
    // Flags were never touched by any of this.
    assert!(lb.list_entries(&all_query()).unwrap().items.iter().all(|e| !e.flagged && e.follow_up.state == FollowUpState::None));
}
