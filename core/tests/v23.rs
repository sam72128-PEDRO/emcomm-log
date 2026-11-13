//! 2.3: operator handover summary, backup health and verification, traffic lifecycle.

mod common;

use common::*;
use emcomm_core::appdb::BackupRun;
use emcomm_core::backup::run_backup;
use emcomm_core::backup_health::{
    self, HealthInputs, HealthState, StepStatus, VerificationRecord, VerificationStore, assess, verify_backup,
};
use emcomm_core::handover::{self, SummaryBasis};
use emcomm_core::lifecycle::{entry_lifecycle, handling_history};
use emcomm_core::links::Relation;
use emcomm_core::logbook::{DB_FILE, Logbook};
use emcomm_core::models::*;
use emcomm_core::snapshot::{self, MANIFEST_FILE};
use std::path::{Path, PathBuf};

fn op(name: &str, call: &str) -> OperatorSnapshot {
    OperatorSnapshot { profile_id: Some(emcomm_core::fsutil::new_uuid()), name: name.into(), callsign: call.into(), ..Default::default() }
}

fn message(subject: &str, h: Option<Handling>) -> EntryInput {
    EntryInput { kind: EntryKind::Message, handling: h, ..input(subject) }
}

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

fn tick() {
    std::thread::sleep(std::time::Duration::from_millis(5));
}

fn revision_count(lb: &Logbook) -> i64 {
    lb.conn.query_row("SELECT COUNT(*) FROM entry_revisions", [], |r| r.get(0)).unwrap()
}

// ------------------------------------------------------------------ traffic lifecycle

#[test]
fn traffic_transmitting_linking_resolving_and_shifts_never_acknowledge() {
    let t = tempfile::tempdir().unwrap();
    let dir = create_book(t.path(), LogbookType::Emergency, "Lifecycle");
    let mut lb = Logbook::open(&dir).unwrap();
    let alice = op("Alice", "KJ4AAA");
    let bob = op("Bob", "KJ4BBB");
    lb.ensure_operator(&alice).unwrap();
    let mut mi = message("Shelter needs cots", Some(Handling::Pending));
    mi.follow_up = FollowUp { state: FollowUpState::Open, note: "Confirm cots delivered".into(), ..Default::default() };
    let m = lb.save_new(&mi, &rid(), &alice).unwrap().entry;
    let other = lb.save_new(&input("Road report"), &rid(), &alice).unwrap().entry;

    // Transmit: Sent, not acknowledged.
    let mut i = as_input(&m);
    i.handling = Some(Handling::Sent);
    let m = lb.save_changes(&m.id, m.current_revision, &i, "Handling changed to Sent", &rid(), &alice).unwrap().entry;
    assert_eq!(m.handling, Some(Handling::Sent));
    let lc = entry_lifecycle(&lb.conn, &m.id).unwrap();
    assert!(lc.acknowledgment.is_none() && lc.outstanding);
    assert_eq!(lc.stage, "Transmitted (no acknowledgment recorded)");

    // Linking, follow-up resolution, favorites, Flag for Report and a shift change leave it alone.
    lb.add_link(&other.id, &m.id, Relation::ResponseTo, "reply heard", &alice).unwrap();
    let mut i = as_input(&m);
    i.follow_up.state = FollowUpState::Resolved;
    i.flagged = true;
    let m = lb.save_changes(&m.id, m.current_revision, &i, "", &rid(), &alice).unwrap().entry;
    lb.set_favorite(&m.id, true).unwrap();
    lb.switch_operator(&bob, None, "").unwrap();
    let m = lb.get_entry(&m.id).unwrap();
    assert_eq!(m.handling, Some(Handling::Sent), "nothing but an explicit save acknowledges");
    assert_eq!(m.follow_up.state, FollowUpState::Resolved);

    // Awaiting ACK then an explicit acknowledgment, recorded by Bob.
    let mut i = as_input(&m);
    i.handling = Some(Handling::AwaitingAck);
    let m = lb.save_changes(&m.id, m.current_revision, &i, "", &rid(), &bob).unwrap().entry;
    let mut i = as_input(&m);
    i.handling = Some(Handling::Acknowledged);
    i.follow_up.state = FollowUpState::Open; // an acknowledged message may still have a follow-up open
    let m = lb.save_changes(&m.id, m.current_revision, &i, "ACK from EOC", &rid(), &bob).unwrap().entry;
    let lc = entry_lifecycle(&lb.conn, &m.id).unwrap();
    let ack = lc.acknowledgment.clone().expect("acknowledgment recorded");
    assert_eq!((ack.by.name.as_str(), ack.reason.as_str(), ack.from), ("Bob", "ACK from EOC", Some(Handling::AwaitingAck)));
    assert!(!lc.outstanding && lc.outstanding_action.is_empty());
    assert_eq!(lc.follow_up_state, FollowUpState::Open);
    assert_eq!(lc.related_count, 1);
    // Attribution of the original entry is unchanged by Bob's edits.
    assert_eq!(m.operator_snapshot.name, "Alice");

    // Going back without a reason is refused; with one it is a recorded correction.
    let mut back = as_input(&m);
    back.handling = Some(Handling::Sent);
    let err = lb.save_changes(&m.id, m.current_revision, &back, "  ", &rid(), &bob).unwrap_err();
    assert_eq!(err.field.as_deref(), Some("reason"));
    let mut clear = as_input(&m);
    clear.handling = None;
    assert!(lb.save_changes(&m.id, m.current_revision, &clear, "", &rid(), &bob).is_err(), "clearing is a regression too");
    let m = lb.save_changes(&m.id, m.current_revision, &back, "ACK was for msg 12, not this one", &rid(), &bob).unwrap().entry;
    let hist = handling_history(&lb.conn, &m.id).unwrap();
    let latest = emcomm_core::lifecycle::latest_handling_changes(&lb.conn, &[m.id.clone(), other.id.clone()]).unwrap();
    assert_eq!(latest.len(), 1, "entries never given a status are absent");
    assert_eq!((latest[0].to, latest[0].by_name.as_str()), (Some(Handling::Sent), "Bob"));
    assert_eq!(latest[0].revision, hist.last().unwrap().revision);
    let path: Vec<_> = hist.iter().map(|c| c.to.unwrap()).collect();
    assert_eq!(path, vec![Handling::Pending, Handling::Sent, Handling::AwaitingAck, Handling::Acknowledged, Handling::Sent]);
    assert!(hist.last().unwrap().correction && !hist[3].correction);
    assert!(entry_lifecycle(&lb.conn, &m.id).unwrap().acknowledgment.is_none());
    // Forward progress never needs a reason.
    let p = lb.save_new(&message("Second", Some(Handling::Pending)), &rid(), &bob).unwrap().entry;
    let mut i = as_input(&p);
    i.handling = Some(Handling::Acknowledged);
    lb.save_changes(&p.id, p.current_revision, &i, "", &rid(), &bob).unwrap();
}

#[test]
fn traffic_pending_counts_match_records() {
    let t = tempfile::tempdir().unwrap();
    let dir = create_book(t.path(), LogbookType::Emergency, "Counts");
    let mut lb = Logbook::open(&dir).unwrap();
    for h in [Handling::Pending, Handling::Pending, Handling::Sent, Handling::AwaitingAck, Handling::Acknowledged, Handling::Relayed] {
        lb.save_new(&message("m", Some(h)), &rid(), &operator()).unwrap();
    }
    let v = lb.save_new(&message("void me", Some(Handling::Pending)), &rid(), &operator()).unwrap().entry;
    lb.void_entry(&v.id, "duplicate", &rid(), &operator()).unwrap();
    let s = lb.stats(None, None).unwrap();
    let outstanding: i64 = s.handling.iter().filter(|(k, _)| Handling::parse(k).unwrap().is_outstanding()).map(|(_, v)| v).sum();
    assert_eq!(outstanding, 4, "pending + sent + awaiting; voided and acknowledged excluded");
    let listed = lb
        .list_entries(&EntryQuery {
            handling: vec![Handling::Pending, Handling::AwaitingAck, Handling::Sent],
            status: "active".into(),
            ..all_query()
        })
        .unwrap()
        .total;
    assert_eq!(listed, outstanding as u32 as i64);
}

// ------------------------------------------------------------------ handover

#[test]
fn handover_rotation_inherited_work_empty_shift_reconstruction_and_archive() {
    let t = tempfile::tempdir().unwrap();
    let dir = create_book(t.path(), LogbookType::Emergency, "Handover");
    let mut lb = Logbook::open(&dir).unwrap();
    // An empty logbook previews cleanly.
    let empty = handover::preview(&lb.conn, None, None, "").unwrap();
    assert_eq!(empty.activity.total, 0);
    assert!(empty.text.contains("No entries were logged"));

    let (alice, bob, cara) = (op("Alice", "KJ4AAA"), op("Bob", "KJ4BBB"), op("Cara", ""));
    lb.ensure_operator(&alice).unwrap();
    let a1 = lb.save_new(&message("Cots for shelter", Some(Handling::AwaitingAck)), &rid(), &alice).unwrap().entry;
    let mut imp = input("Bridge out on CR-12");
    imp.important = true;
    imp.follow_up =
        FollowUp { state: FollowUpState::Open, due_utc: Some("2020-01-01T00:00:00Z".into()), note: "Ask DOT".into(), ..Default::default() };
    lb.save_new(&imp, &rid(), &alice).unwrap();
    lb.save_new(&input("Check-in"), &rid(), &alice).unwrap();
    tick();
    lb.switch_operator(&bob, None, "Two messages need ACK").unwrap();
    tick();
    let b1 = lb.save_new(&message("Water request", Some(Handling::Pending)), &rid(), &bob).unwrap().entry;
    let mut pri = input("Medical evac requested");
    pri.priority = Priority::Emergency;
    lb.save_new(&pri, &rid(), &bob).unwrap();

    let marker = snapshot::data_marker(&lb.conn).unwrap();
    let revs = revision_count(&lb);
    let p = handover::preview(&lb.conn, Some(cara.clone()), None, "Watch CR-12").unwrap();
    assert_eq!(snapshot::data_marker(&lb.conn).unwrap(), marker, "a preview changes nothing");
    assert_eq!(revision_count(&lb), revs);

    assert_eq!(p.basis, SummaryBasis::Preview);
    assert_eq!(p.outgoing.as_ref().unwrap().name, "Bob");
    assert_eq!(p.incoming.as_ref().unwrap().name, "Cara");
    assert_eq!(p.shift_start_note, "Two messages need ACK");
    assert_eq!(p.handover_note, "Watch CR-12");
    assert_eq!(p.activity.total, 2, "only Bob's shift");
    assert_eq!(p.activity.significant.total, 1);
    let o = p.outstanding.as_ref().unwrap();
    assert_eq!(o.awaiting_ack.total, 1);
    assert!(o.awaiting_ack.items[0].inherited && o.awaiting_ack.items[0].id == a1.id, "Alice's message is the station's, inherited");
    assert_eq!(o.not_transmitted.total, 1);
    assert!(!o.not_transmitted.items[0].inherited && o.not_transmitted.items[0].id == b1.id);
    assert_eq!((o.open_follow_ups.total, o.overdue_follow_ups), (1, 1));
    assert!(o.open_follow_ups.items[0].overdue);
    assert_eq!(o.important.total, 1);
    assert_eq!(o.inherited_items, 2);
    assert!(p.text.contains("OUTSTANDING AT THE STATION") && p.text.contains("from earlier shift"));

    // The switch itself does not depend on the summary; Cara's shift starts empty.
    tick();
    lb.switch_operator(&cara, None, "Watch CR-12").unwrap();
    let c = handover::preview(&lb.conn, None, None, "").unwrap();
    assert_eq!(c.activity.total, 0);
    assert!(c.text.contains("No entries were logged during this shift"));
    assert_eq!(c.outstanding.as_ref().unwrap().awaiting_ack.total, 1, "open work stays visible across shifts");
    assert_eq!(c.outstanding.as_ref().unwrap().inherited_items, 3, "counted per entry");

    // Past shifts are reconstructed and say so; outstanding work is not recalculated for them.
    let sessions = lb.operator_sessions().unwrap();
    assert_eq!(sessions.len(), 3);
    let ra = handover::reconstruct(&lb.conn, &sessions[0].id).unwrap();
    assert_eq!(ra.basis, SummaryBasis::Reconstructed);
    assert!(ra.outstanding.is_none());
    assert_eq!(ra.activity.total, 3);
    assert_eq!(ra.incoming.as_ref().unwrap().name, "Bob");
    assert_eq!(ra.handover_note, "Two messages need ACK");
    assert!(ra.text.contains("RECONSTRUCTED") && ra.text.contains("Not an original snapshot"));
    assert!(handover::reconstruct(&lb.conn, &sessions[2].id).is_err(), "the open shift is previewed, not reconstructed");

    // Entries keep their original operator.
    let names: Vec<String> = lb.list_entries(&all_query()).unwrap().items.iter().map(|e| e.operator_snapshot.name.clone()).collect();
    assert_eq!(names.iter().filter(|n| *n == "Alice").count(), 3);
    assert_eq!(names.iter().filter(|n| *n == "Bob").count(), 2);

    // Archived: the summary still reads; switching stays blocked.
    lb.set_archived(true).unwrap();
    assert!(handover::preview(&lb.conn, None, None, "").is_ok());
    assert!(lb.switch_operator(&alice, None, "").is_err());
}

// ------------------------------------------------------------------ backups

fn backup_with_attachment(t: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let dir = create_book(t, LogbookType::General, "Backups");
    let mut lb = Logbook::open(&dir).unwrap();
    let a = lb.stage_attachment_bytes("map.png", b"\x89PNG\r\n\x1a\nmap", "paste").unwrap();
    lb.save_new(&EntryInput { new_attachments: vec![a], ..input("With map") }, &rid(), &operator()).unwrap();
    lb.save_new(&input("Plain"), &rid(), &operator()).unwrap();
    drop(lb);
    let dest = t.join("dest");
    std::fs::create_dir(&dest).unwrap();
    let r = run_backup(&dir, &dest, "manual", &BackupSettings::default()).unwrap();
    (dir, dest, PathBuf::from(r.path))
}

fn tree_digest(dir: &Path) -> Vec<(String, u64, String, std::time::SystemTime)> {
    let mut out = vec![];
    fn walk(base: &Path, d: &Path, out: &mut Vec<(String, u64, String, std::time::SystemTime)>) {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                let md = e.metadata().unwrap();
                let (h, n) = emcomm_core::fsutil::sha256_file(&p).unwrap();
                out.push((p.strip_prefix(base).unwrap().to_string_lossy().to_string(), n, h, md.modified().unwrap()));
            }
        }
    }
    walk(dir, dir, &mut out);
    out.sort();
    out
}

#[test]
fn verify_valid_backup_is_complete_and_read_only() {
    let t = tempfile::tempdir().unwrap();
    let (_dir, _dest, b) = backup_with_attachment(t.path());
    let before = tree_digest(&b);
    let r = verify_backup(&b);
    assert!(r.complete, "{:?}", r.problems);
    assert!(r.steps.iter().all(|s| s.status == StepStatus::Passed));
    assert_eq!(r.steps.len(), 7);
    assert_eq!((r.files_checked, r.attachments_checked, r.entry_count), (2, 1, Some(2)));
    assert!(r.scope.starts_with("Complete verification"));
    assert_eq!(tree_digest(&b), before, "verification never changes the backup");
}

#[test]
fn verify_detects_corruption_missing_attachments_bad_manifest_and_missing_folder() {
    let t = tempfile::tempdir().unwrap();
    let (dir, dest, b) = backup_with_attachment(t.path());
    let failed = |r: &backup_health::VerifyReport, id: &str| r.steps.iter().find(|s| s.id == id).unwrap().status == StepStatus::Failed;

    // A modified database byte.
    let db = b.join(DB_FILE);
    let mut bytes = std::fs::read(&db).unwrap();
    let n = bytes.len();
    bytes[n - 10] ^= 0xFF;
    std::fs::write(&db, &bytes).unwrap();
    let r = verify_backup(&b);
    assert!(!r.complete && failed(&r, "hashes"));
    assert!(!r.scope.starts_with("Complete"));

    // A missing attachment is named.
    let r2 = run_backup(&dir, &dest, "manual", &BackupSettings { keep_recent: 10, ..Default::default() }).unwrap();
    let b2 = PathBuf::from(r2.path);
    let att = std::fs::read_dir(b2.join("attachments")).unwrap().flatten().next().unwrap().path();
    std::fs::remove_file(&att).unwrap();
    let r = verify_backup(&b2);
    assert!(!r.complete && failed(&r, "hashes") && failed(&r, "attachments"));
    assert!(r.problems.iter().any(|p| p.contains("map.png")), "{:?}", r.problems);

    // A truncated database fails the SQLite check (or cannot be opened).
    let r3 = run_backup(&dir, &dest, "manual", &BackupSettings { keep_recent: 10, ..Default::default() }).unwrap();
    let b3 = PathBuf::from(r3.path);
    let raw = std::fs::read(b3.join(DB_FILE)).unwrap();
    std::fs::write(b3.join(DB_FILE), &raw[..raw.len() / 2]).unwrap();
    let r = verify_backup(&b3);
    assert!(!r.complete && failed(&r, "sqlite"));

    // A broken manifest: later steps are reported as not run, never as passed.
    let r4 = run_backup(&dir, &dest, "manual", &BackupSettings { keep_recent: 10, ..Default::default() }).unwrap();
    let b4 = PathBuf::from(r4.path);
    std::fs::write(b4.join(MANIFEST_FILE), b"{not json").unwrap();
    let r = verify_backup(&b4);
    assert!(failed(&r, "manifest"));
    assert!(r.steps.iter().skip(2).all(|s| s.status == StepStatus::NotRun));

    // An extra unlisted file.
    let r5 = run_backup(&dir, &dest, "manual", &BackupSettings { keep_recent: 10, ..Default::default() }).unwrap();
    let b5 = PathBuf::from(r5.path);
    std::fs::write(b5.join("attachments").join("stray.bin"), b"x").unwrap();
    assert!(failed(&verify_backup(&b5), "hashes"));

    // A folder that is gone.
    let r = verify_backup(&t.path().join("nope"));
    assert!(failed(&r, "access") && !r.complete);
}

fn inputs<'a>(
    dest: &'a str,
    runs: &'a [BackupRun],
    v: &'a [VerificationRecord],
    latest: backup_health::LatestBackup,
    marker: Option<String>,
    now: chrono::DateTime<chrono::Utc>,
) -> HealthInputs<'a> {
    HealthInputs {
        destination: Some(dest),
        destination_check: Some(Ok(())),
        runs,
        latest_backup: Some(latest),
        verifications: v,
        live_marker: marker,
        last_unchanged_check_utc: None,
        interval_minutes: 15,
        on_close: true,
        now,
    }
}

fn leak<T>(v: Vec<T>) -> &'static [T] {
    Box::leak(v.into_boxed_slice())
}

fn run(ok: bool, at: &str, path: Option<&str>, marker: Option<&str>, msg: &str) -> BackupRun {
    BackupRun {
        logbook_id: "L".into(),
        kind: "auto".into(),
        started_at_utc: at.into(),
        finished_at_utc: at.into(),
        ok,
        path: path.map(String::from),
        entry_count: Some(1),
        message: msg.into(),
        data_marker: marker.map(String::from),
    }
}

#[test]
fn health_states() {
    let t = tempfile::tempdir().unwrap();
    let (dir, dest, b) = backup_with_attachment(t.path());
    let lbid = Logbook::open(&dir).unwrap().id.clone();
    let latest = backup_health::latest_backup(&dest, &lbid).expect("latest on disk");
    assert_eq!(latest.path, b.to_string_lossy());
    let live = snapshot::live_data_marker(&dir).unwrap();
    let created = latest.created_at_utc.clone();
    let now = emcomm_core::validate::parse_utc(&created).unwrap() + chrono::Duration::minutes(3);
    let bp = b.to_string_lossy().to_string();
    let runs = vec![run(true, &created, Some(&bp), Some(&live), "Verified")];
    let dests = dest.to_string_lossy().to_string();
    let base = |runs: &'static [BackupRun], v: &'static [VerificationRecord], marker: Option<String>, now| {
        inputs(Box::leak(dests.clone().into_boxed_str()), runs, v, latest.clone(), marker, now)
    };
    // Not configured / unavailable.
    let h = assess(HealthInputs {
        destination: None,
        destination_check: None,
        latest_backup: None,
        ..base(leak(runs.clone()), &[], None, now)
    });
    assert_eq!(h.state, HealthState::NotConfigured);
    let gone = t.path().join("unplugged").to_string_lossy().to_string();
    let h = assess(HealthInputs { destination: Some(&gone), latest_backup: None, ..base(leak(runs.clone()), &[], None, now) });
    assert_eq!(h.state, HealthState::Unavailable);
    assert_eq!(h.destination_accessible, Some(false));

    // Exists but not verified since creation: Unknown, never Verified.
    let h = assess(base(leak(runs.clone()), &[], Some(live.clone()), now));
    assert_eq!(h.state, HealthState::Unknown);
    assert_eq!(h.changes_since_backup, Some(false));

    // Verified after a full verification of that same folder.
    let rep = verify_backup(&b);
    let ok = VerificationRecord::from_report(&lbid, &rep);
    let h = assess(base(leak(runs.clone()), leak(vec![ok.clone()]), Some(live.clone()), now));
    assert_eq!(h.state, HealthState::Verified);
    assert_eq!(h.latest_verification.unwrap().verified_at_utc, rep.verified_at_utc);
    // A verification of a different (older) backup does not verify the latest one.
    let other = VerificationRecord { path: "/elsewhere".into(), ..ok.clone() };
    assert_eq!(assess(base(leak(runs.clone()), leak(vec![other]), Some(live.clone()), now)).state, HealthState::Unknown);

    // Verification failed.
    let bad = VerificationRecord { complete: false, problems: vec!["File does not match".into()], ..ok.clone() };
    assert_eq!(assess(base(leak(runs.clone()), leak(vec![bad]), Some(live.clone()), now)).state, HealthState::VerificationFailed);

    // A failed attempt after the last success needs attention.
    let mut failed = runs.clone();
    failed.insert(0, run(false, &created, None, None, "Destination full"));
    let h = assess(base(leak(failed), leak(vec![ok.clone()]), Some(live.clone()), now));
    assert_eq!(h.state, HealthState::AttentionRequired);
    assert!(h.headline.contains("Destination full"));
    assert_eq!(h.last_error.as_deref().map(|e| e.starts_with("Destination full")), Some(true));

    // Overdue only when data changed; skipped-because-unchanged is never a failure.
    let late = now + chrono::Duration::hours(3);
    let h = assess(base(leak(runs.clone()), &[], Some("changed".into()), late));
    assert_eq!(h.state, HealthState::AttentionRequired);
    assert!(h.headline.contains("overdue"));
    let h = assess(HealthInputs {
        last_unchanged_check_utc: Some("2026-11-13T10:00:00.000Z".into()),
        ..base(leak(runs.clone()), leak(vec![ok.clone()]), Some(live.clone()), late)
    });
    assert_eq!(h.state, HealthState::Verified, "an unchanged logbook is not overdue");

    // No backup at the destination yet.
    let h = assess(HealthInputs { latest_backup: None, ..base(&[], &[], Some(live), now) });
    assert_eq!(h.state, HealthState::AttentionRequired);
}

#[test]
fn verification_store_keeps_recent_records_outside_backups() {
    let t = tempfile::tempdir().unwrap();
    let store = VerificationStore::new(t.path());
    assert!(store.for_logbook("L").is_empty());
    for i in 0..13 {
        store
            .record(VerificationRecord {
                logbook_id: "L".into(),
                path: format!("/b/{i}"),
                verified_at_utc: format!("2026-11-13T10:{i:02}:00.000Z"),
                complete: true,
                scope: "x".into(),
                problems: vec![],
                backup_created_at_utc: None,
            })
            .unwrap();
    }
    let v = store.for_logbook("L");
    assert_eq!(v.len(), 10);
    assert_eq!(v[0].path, "/b/12");
    std::fs::write(t.path().join(backup_health::VERIFICATIONS_FILE), b"garbage").unwrap();
    assert!(store.for_logbook("L").is_empty(), "a damaged record file never breaks the app");
}

#[test]
fn backup_marker_sees_links_directory_and_shift_changes() {
    use emcomm_core::directory::DirectoryInput;
    let t = tempfile::tempdir().unwrap();
    let dir = create_book(t.path(), LogbookType::General, "Marker");
    let mut lb = Logbook::open(&dir).unwrap();
    let a = lb.save_new(&input("a"), &rid(), &operator()).unwrap().entry;
    let b = lb.save_new(&input("b"), &rid(), &operator()).unwrap().entry;
    let m0 = snapshot::data_marker(&lb.conn).unwrap();
    let l = lb.add_link(&a.id, &b.id, Relation::RelatedTo, "", &operator()).unwrap();
    let m1 = snapshot::data_marker(&lb.conn).unwrap();
    assert_ne!(m0, m1, "a new link is a change worth backing up");
    lb.save_directory(None, &DirectoryInput { label: "EOC".into(), ..Default::default() }).unwrap();
    let m2 = snapshot::data_marker(&lb.conn).unwrap();
    assert_ne!(m1, m2, "a directory change is a change worth backing up");
    lb.switch_operator(&op("Other", ""), None, "").unwrap();
    let m3 = snapshot::data_marker(&lb.conn).unwrap();
    assert_ne!(m2, m3);
    let _ = l;
    assert_eq!(m3, snapshot::data_marker(&lb.conn).unwrap(), "stable when nothing changed");
}
