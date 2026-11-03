//! Integration tests for the live-storage contract (A02-A07) against real SQLite files.

mod common;

use common::*;
use emcomm_core::appdb::AppDb;
use emcomm_core::drafts::{DraftMeta, DraftStore};
use emcomm_core::error::ErrorCode;
use emcomm_core::faults::{self, Fault};
use emcomm_core::fsutil::new_uuid;
use emcomm_core::logbook::{effective_pragmas, Logbook, DB_FILE};
use emcomm_core::models::*;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};

static FAULT_LOCK: Mutex<()> = Mutex::new(());

// ---------- A02 ----------

#[test]
fn a02_each_type_from_preset_keeps_independent_settings() {
    let app = tempfile::tempdir().unwrap();
    let parent = tempfile::tempdir().unwrap();
    for ty in [LogbookType::General, LogbookType::Ham, LogbookType::Emergency] {
        let (mut db, preset) = builtin_preset(app.path(), ty);
        let dir = create_book_with_preset(parent.path(), ty, &format!("Book {}", ty.as_str()), Some(preset.clone()));
        let lb = Logbook::open(&dir).unwrap();
        let before = lb.settings().unwrap();
        let templates_before = lb.templates().unwrap();
        assert_eq!(before.preset_id.as_deref(), Some(preset.id.as_str()));
        assert_eq!(before.preset_version, preset.version);
        // Edit the preset afterwards: the logbook must not change.
        let mut cfg = preset.config.clone();
        cfg.quick_tags = vec!["CHANGED".into()];
        cfg.templates.clear();
        let edited = db
            .update_preset(&preset.id, &PresetInput { name: "Edited".into(), logbook_type: Some(ty), profile_id: None, config: cfg })
            .unwrap();
        assert_eq!(edited.version, preset.version + 1);
        drop(lb);
        let lb = Logbook::open(&dir).unwrap();
        assert_eq!(lb.settings().unwrap(), before);
        assert_eq!(lb.templates().unwrap(), templates_before);
        assert_eq!(lb.info().unwrap().logbook_type, ty);
    }
}

// ---------- A03 ----------

#[test]
fn a03_save_entry_and_changes_persist_at_live_path_without_extra_save() {
    let parent = tempfile::tempdir().unwrap();
    let dir = create_book(parent.path(), LogbookType::General, "Durability");
    let (id, saved) = {
        let mut lb = Logbook::open(&dir).unwrap();
        let (mode, sync, fk) = effective_pragmas(&lb.conn).unwrap();
        assert_eq!((mode.as_str(), sync, fk), ("wal", 2, 1));
        let r = lb.save_new(&input("Bridge out"), &rid(), &operator()).unwrap();
        assert_eq!(r.entry.sequence_no, 1);
        assert_eq!(r.entry.current_revision, 1);
        let mut edit = input("Bridge out on CR-12");
        edit.body = "Edited body ✓ ünïcödé".into();
        let c = lb.save_changes(&r.entry.id, 1, &edit, "clarified location", &rid(), &operator()).unwrap();
        assert_eq!(c.entry.current_revision, 2);
        assert_eq!(c.entry.sequence_no, 1);
        // Drop without close(): no checkpoint, no "save logbook" step.
        (r.entry.id, c.entry)
    };
    // Read the live file directly with an independent connection.
    let raw = rusqlite::Connection::open(dir.join(DB_FILE)).unwrap();
    let (subject, body, rev): (String, String, i64) = raw
        .query_row("SELECT subject, body, current_revision FROM entries WHERE id=?1", [&id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap();
    assert_eq!((subject.as_str(), body.as_str(), rev), ("Bridge out on CR-12", "Edited body ✓ ünïcödé", 2));
    drop(raw);
    let lb = Logbook::open(&dir).unwrap();
    let e = lb.get_entry(&id).unwrap();
    assert_eq!(e, saved);
    let revs = lb.revisions(&id).unwrap();
    assert_eq!(revs.len(), 2);
    assert_eq!(revs[1].reason, "clarified location");
    assert_eq!(revs[1].before.as_ref().unwrap()["subject"], "Bridge out");
}

#[test]
#[ignore = "child process helper for a03_committed_entry_survives_process_kill"]
fn child_save_then_abort() {
    let Ok(dir) = std::env::var("EMCOMM_CHILD_DIR") else { return };
    let mut lb = Logbook::open(Path::new(&dir)).unwrap();
    let r = lb.save_new(&input("crash survivor"), "7f0d8b2e-0000-4000-8000-000000000001", &operator()).unwrap();
    println!("SAVED {} {}", r.entry.sequence_no, r.entry.id);
    use std::io::Write;
    std::io::stdout().flush().unwrap();
    std::process::abort();
}

#[test]
fn a03_committed_entry_survives_process_kill() {
    let parent = tempfile::tempdir().unwrap();
    let dir = create_book(parent.path(), LogbookType::General, "Crash");
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "child_save_then_abort", "--nocapture", "--test-threads=1"])
        .env("EMCOMM_CHILD_DIR", &dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "child should have aborted");
    assert!(stdout.contains("SAVED 1"), "child output: {stdout}");
    let lb = Logbook::open(&dir).unwrap();
    assert!(lb.notices.iter().any(|n| n.contains("not closed cleanly")), "stale lock should trigger integrity check");
    let page = lb.list_entries(&all_query()).unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].subject, "crash survivor");
    assert_eq!(page.items[0].current_revision, 1);
    assert_eq!(lb.info().unwrap().next_sequence, 2);
}

// ---------- A04 ----------

#[test]
fn a04_concurrent_saves_get_unique_monotone_numbers() {
    let parent = tempfile::tempdir().unwrap();
    let dir = create_book(parent.path(), LogbookType::General, "Concurrency");
    // Truly concurrent writers: independent connections to the same file.
    let mut handles = Vec::new();
    for t in 0..4 {
        let dir = dir.clone();
        handles.push(std::thread::spawn(move || {
            let mut lb = Logbook::open_unlocked_for_test(&dir).unwrap();
            let mut nums = Vec::new();
            for i in 0..25 {
                let r = lb.save_new(&input(&format!("t{t} #{i}")), &rid(), &operator()).unwrap();
                nums.push(r.entry.sequence_no);
            }
            nums
        }));
    }
    let mut all: Vec<i64> = handles.into_iter().flat_map(|h| h.join().unwrap()).collect();
    all.sort();
    assert_eq!(all, (1..=100).collect::<Vec<_>>());
    // Shared app handle across threads (how the app actually serializes commands).
    let lb = Arc::new(Mutex::new(Logbook::open(&dir).unwrap()));
    let hs: Vec<_> = (0..4)
        .map(|_| {
            let lb = lb.clone();
            std::thread::spawn(move || (0..10).map(|_| lb.lock().unwrap().save_new(&input("x"), &rid(), &operator()).unwrap().entry.sequence_no).collect::<Vec<_>>())
        })
        .collect();
    let mut more: Vec<i64> = hs.into_iter().flat_map(|h| h.join().unwrap()).collect();
    more.sort();
    assert_eq!(more, (101..=140).collect::<Vec<_>>());
}

#[test]
fn a04_retry_token_never_duplicates_and_void_never_reuses_numbers() {
    let _g = FAULT_LOCK.lock().unwrap();
    let parent = tempfile::tempdir().unwrap();
    let dir = create_book(parent.path(), LogbookType::General, "Numbers");
    let mut lb = Logbook::open(&dir).unwrap();
    let token = rid();
    let a = lb.save_new(&input("first"), &token, &operator()).unwrap();
    let again = lb.save_new(&input("first (retry after uncertain response)"), &token, &operator()).unwrap();
    assert!(again.duplicate);
    assert_eq!(again.entry.id, a.entry.id);
    assert_eq!(again.entry.subject, "first");
    let b = lb.save_new(&input("second"), &rid(), &operator()).unwrap();
    assert_eq!(b.entry.sequence_no, 2);
    lb.void_entry(&b.entry.id, "duplicate", &rid(), &operator()).unwrap();
    // A rolled-back attempt does not consume a number.
    faults::inject("entry.save.before_commit", Fault::Io, 1);
    assert!(lb.save_new(&input("fails"), &rid(), &operator()).is_err());
    faults::clear();
    let c = lb.save_new(&input("third"), &rid(), &operator()).unwrap();
    assert_eq!(c.entry.sequence_no, 3, "voided #2 is not reused and the failed attempt consumed nothing");
    lb.restore_entry(&b.entry.id, "", &rid(), &operator()).unwrap();
    let d = lb.save_new(&input("fourth"), &rid(), &operator()).unwrap();
    assert_eq!(d.entry.sequence_no, 4);
    // Idempotent edits
    let tok = rid();
    let e1 = lb.save_changes(&a.entry.id, 1, &input("first edited"), "", &tok, &operator()).unwrap();
    let e2 = lb.save_changes(&a.entry.id, 1, &input("first edited"), "", &tok, &operator()).unwrap();
    assert!(e2.duplicate);
    assert_eq!(e1.entry.current_revision, 2);
    assert_eq!(lb.revisions(&a.entry.id).unwrap().len(), 2);
    // Stale base revision is rejected, not silently overwritten.
    let stale = lb.save_changes(&a.entry.id, 1, &input("stale"), "", &rid(), &operator()).unwrap_err();
    assert_eq!(stale.code, ErrorCode::Conflict);
}

// ---------- A05 ----------

fn assert_no_entry_and_sequence(lb: &Logbook, expected_total: i64, expected_next: i64) {
    assert_eq!(lb.list_entries(&all_query()).unwrap().total, expected_total);
    assert_eq!(lb.info().unwrap().next_sequence, expected_next);
}

#[test]
fn a05_injected_disk_full_permission_io_busy_fail_honestly() {
    let _g = FAULT_LOCK.lock().unwrap();
    let parent = tempfile::tempdir().unwrap();
    let dir = create_book(parent.path(), LogbookType::General, "Faults");
    let mut lb = Logbook::open(&dir).unwrap();
    lb.save_new(&input("ok"), &rid(), &operator()).unwrap();
    for (fault, code) in [
        (Fault::DiskFull, ErrorCode::DiskFull),
        (Fault::PermissionDenied, ErrorCode::PermissionDenied),
        (Fault::Io, ErrorCode::Io),
        (Fault::Busy, ErrorCode::Busy),
    ] {
        faults::inject("entry.save.before_commit", fault, 1);
        let e = lb.save_new(&input("should not persist"), &rid(), &operator()).unwrap_err();
        assert_eq!(e.code, code);
        assert_no_entry_and_sequence(&lb, 1, 2);
    }
    faults::clear();
    let ok = lb.save_new(&input("after recovery"), &rid(), &operator()).unwrap();
    assert_eq!(ok.entry.sequence_no, 2);
}

#[test]
fn a05_real_sqlite_full_rolls_back() {
    let parent = tempfile::tempdir().unwrap();
    let dir = create_book(parent.path(), LogbookType::General, "Full");
    let mut lb = Logbook::open(&dir).unwrap();
    lb.save_new(&input("before full"), &rid(), &operator()).unwrap();
    let pages: i64 = lb.conn.query_row("PRAGMA page_count", [], |r| r.get(0)).unwrap();
    lb.conn.execute_batch(&format!("PRAGMA max_page_count={pages}")).unwrap();
    let mut big = input("big");
    big.body = "x".repeat(90_000);
    let e = lb.save_new(&big, &rid(), &operator()).unwrap_err();
    assert_eq!(e.code, ErrorCode::DiskFull, "{e:?}");
    lb.conn.execute_batch("PRAGMA max_page_count=1073741823").unwrap();
    assert_no_entry_and_sequence(&lb, 1, 2);
}

#[test]
fn a05_missing_drive_blocks_final_saves_and_draft_survives() {
    let parent = tempfile::tempdir().unwrap();
    let app = tempfile::tempdir().unwrap();
    let dir = create_book(parent.path(), LogbookType::General, "Unplugged");
    let mut lb = Logbook::open(&dir).unwrap();
    lb.save_new(&input("before unplug"), &rid(), &operator()).unwrap();
    let moved = parent.path().join("moved-away");
    std::fs::rename(&dir, &moved).unwrap();
    let drafts = DraftStore::new(app.path());
    let meta = DraftMeta {
        draft_id: new_uuid(),
        logbook_id: lb.id.clone(),
        logbook_name: "Unplugged".into(),
        editor: "new".into(),
        entry_id: None,
        entry_sequence: None,
        base_revision: None,
    };
    let content = serde_json::to_value(input("typed while unplugged")).unwrap();
    let e = lb.save_new(&input("typed while unplugged"), &rid(), &operator()).unwrap_err();
    assert_eq!(e.code, ErrorCode::StorageUnavailable);
    assert!(lb.health().unwrap().error.is_some());
    // Local draft store still works, so the operator's text is recoverable.
    drafts.save(&meta, &content).unwrap();
    assert_eq!(drafts.list(&lb.id).unwrap()[0].content["subject"], "typed while unplugged");
    // Nothing was written to the moved database by the blocked save.
    let raw = rusqlite::Connection::open(moved.join(DB_FILE)).unwrap();
    let n: i64 = raw.query_row("SELECT COUNT(*) FROM entries", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
    drop(lb);
    // Reconnect ("plug back in") and retry the same request: it succeeds once.
    std::fs::rename(&moved, &dir).unwrap();
    let mut lb = Logbook::open(&dir).unwrap();
    lb.save_new(&input("typed while unplugged"), &rid(), &operator()).unwrap();
    assert_eq!(lb.list_entries(&all_query()).unwrap().total, 2);
}

#[test]
fn a05_database_busy_reports_busy() {
    let parent = tempfile::tempdir().unwrap();
    let dir = create_book(parent.path(), LogbookType::General, "Busy");
    let mut lb = Logbook::open(&dir).unwrap();
    lb.conn.busy_timeout(std::time::Duration::from_millis(200)).unwrap();
    let other = rusqlite::Connection::open(dir.join(DB_FILE)).unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    let e = lb.save_new(&input("blocked"), &rid(), &operator()).unwrap_err();
    assert_eq!(e.code, ErrorCode::Busy);
    other.execute_batch("ROLLBACK").unwrap();
    assert_eq!(lb.save_new(&input("unblocked"), &rid(), &operator()).unwrap().entry.sequence_no, 1);
}

#[test]
fn a05_second_process_cannot_open_same_logbook() {
    let parent = tempfile::tempdir().unwrap();
    let dir = create_book(parent.path(), LogbookType::General, "Locked");
    let _lb = Logbook::open(&dir).unwrap();
    let e = Logbook::open(&dir).err().unwrap();
    assert_eq!(e.code, ErrorCode::Locked);
}

#[cfg(unix)]
#[test]
fn a05_permission_denied_on_attachment_write() {
    use std::os::unix::fs::PermissionsExt;
    let parent = tempfile::tempdir().unwrap();
    let dir = create_book(parent.path(), LogbookType::General, "Perms");
    let mut lb = Logbook::open(&dir).unwrap();
    let src = parent.path().join("photo.jpg");
    std::fs::write(&src, b"jpeg bytes").unwrap();
    let staged = lb.stage_attachment(&src).unwrap();
    let att = dir.join("attachments");
    std::fs::set_permissions(&att, std::fs::Permissions::from_mode(0o555)).unwrap();
    let mut i = input("with photo");
    i.new_attachments = vec![staged.clone()];
    let e = lb.save_new(&i, &rid(), &operator()).unwrap_err();
    std::fs::set_permissions(&att, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(e.code, ErrorCode::PermissionDenied, "{e:?}");
    assert_eq!(lb.list_entries(&all_query()).unwrap().total, 0);
    // Staged file is still there, so retry works without re-attaching.
    let ok = lb.save_new(&i, &rid(), &operator()).unwrap();
    assert_eq!(ok.entry.attachments.len(), 1);
    assert_eq!(ok.entry.attachments[0].sha256, staged.sha256);
    assert!(dir.join(&ok.entry.attachments[0].rel_path).is_file());
}

#[test]
fn a05_archived_logbook_is_read_only() {
    let parent = tempfile::tempdir().unwrap();
    let dir = create_book(parent.path(), LogbookType::General, "Archive");
    let mut lb = Logbook::open(&dir).unwrap();
    lb.set_archived(true).unwrap();
    assert_eq!(lb.save_new(&input("no"), &rid(), &operator()).unwrap_err().code, ErrorCode::ReadOnly);
    drop(lb);
    let mut lb = Logbook::open(&dir).unwrap();
    assert!(lb.read_only);
    lb.set_archived(false).unwrap();
    lb.save_new(&input("yes"), &rid(), &operator()).unwrap();
}

// ---------- A06 ----------

#[test]
#[ignore = "child process helper for a06_draft_recovery_after_kill"]
fn child_write_draft_then_abort() {
    let (Ok(app), Ok(lbid)) = (std::env::var("EMCOMM_CHILD_APP"), std::env::var("EMCOMM_CHILD_LB")) else { return };
    let store = DraftStore::new(Path::new(&app));
    let meta = DraftMeta {
        draft_id: "3d6f6a0e-1111-4222-8333-444444444444".into(),
        logbook_id: lbid,
        logbook_name: "Crashy".into(),
        editor: "new".into(),
        entry_id: None,
        entry_sequence: None,
        base_revision: None,
    };
    let at = store.save(&meta, &serde_json::json!({"subject": "unfinished welfare check", "body": "Family at 1247 Pine Rd"})).unwrap();
    println!("DRAFT {at}");
    use std::io::Write;
    std::io::stdout().flush().unwrap();
    std::process::abort();
}

#[test]
fn a06_draft_recovery_after_kill_does_not_finalize() {
    let parent = tempfile::tempdir().unwrap();
    let app = tempfile::tempdir().unwrap();
    let dir = create_book(parent.path(), LogbookType::Emergency, "Crashy");
    let lbid = Logbook::open(&dir).unwrap().id.clone();
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "child_write_draft_then_abort", "--nocapture", "--test-threads=1"])
        .env("EMCOMM_CHILD_APP", app.path())
        .env("EMCOMM_CHILD_LB", &lbid)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let saved_at = stdout
        .lines()
        .find_map(|l| l.find("DRAFT ").map(|i| l[i + 6..].trim().to_string()))
        .unwrap_or_else(|| panic!("child wrote no draft: {stdout}"));
    let store = DraftStore::new(app.path());
    let drafts = store.list(&lbid).unwrap();
    assert_eq!(drafts.len(), 1);
    assert!(drafts[0].intact);
    assert_eq!(drafts[0].saved_at_utc, saved_at, "recovery offer shows the accurate time");
    assert_eq!(drafts[0].meta.logbook_name, "Crashy");
    let lb = Logbook::open(&dir).unwrap();
    assert_eq!(lb.list_entries(&all_query()).unwrap().total, 0, "a draft is never a finalized entry");
    // "Keep for later" leaves it; "Discard" removes only that draft.
    store.delete(&lbid, &drafts[0].meta.draft_id).unwrap();
    assert!(store.list(&lbid).unwrap().is_empty());
}

// ---------- A07 ----------

#[test]
fn a07_edit_void_restore_timeline_and_filters() {
    let parent = tempfile::tempdir().unwrap();
    let dir = create_book(parent.path(), LogbookType::General, "Timeline");
    let mut lb = Logbook::open(&dir).unwrap();
    let a = lb.save_new(&input("original"), &rid(), &operator()).unwrap().entry;
    lb.save_new(&input("other"), &rid(), &operator()).unwrap();
    lb.save_changes(&a.id, 1, &input("edited"), "typo", &rid(), &operator()).unwrap();
    lb.void_entry(&a.id, "wrong net", &rid(), &operator()).unwrap();
    let def = lb.list_entries(&EntryQuery::default()).unwrap();
    assert_eq!(def.total, 1, "default list hides voided");
    let voids = lb.list_entries(&EntryQuery { status: "void".into(), ..Default::default() }).unwrap();
    assert_eq!(voids.items[0].id, a.id);
    assert_eq!(voids.items[0].void_reason.as_deref(), Some("wrong net"));
    let edit_voided = lb.save_changes(&a.id, 3, &input("nope"), "", &rid(), &operator()).unwrap_err();
    assert_eq!(edit_voided.code, ErrorCode::Validation);
    lb.restore_entry(&a.id, "net confirmed", &rid(), &operator()).unwrap();
    let revs = lb.revisions(&a.id).unwrap();
    let actions: Vec<_> = revs.iter().map(|r| r.action.as_str()).collect();
    assert_eq!(actions, ["create", "edit", "void", "restore"]);
    assert_eq!(revs.iter().map(|r| r.revision).collect::<Vec<_>>(), [1, 2, 3, 4]);
    assert_eq!(revs[2].after["status"], "void");
    assert_eq!(revs[3].after["status"], "active");
    assert_eq!(revs[1].after["subject"], "edited");
    let e = lb.get_entry(&a.id).unwrap();
    assert_eq!((e.sequence_no, e.current_revision, e.status), (1, 4, EntryStatus::Active));
    // Revisions are immutable at the database level.
    assert!(lb.conn.execute("UPDATE entry_revisions SET reason='x'", []).is_err());
    assert!(lb.conn.execute("DELETE FROM entry_revisions", []).is_err());
    // An unchanged Save Changes adds no revision.
    let same = lb.save_changes(&a.id, 4, &input("edited"), "", &rid(), &operator()).unwrap();
    assert!(same.unchanged);
}

#[test]
fn search_filters_and_fts_stay_in_sync() {
    let parent = tempfile::tempdir().unwrap();
    let dir = create_book(parent.path(), LogbookType::General, "Search");
    let mut lb = Logbook::open(&dir).unwrap();
    let a = lb.save_new(&input("Shelter capacity update"), &rid(), &operator()).unwrap().entry;
    let mut w = input("Welfare check");
    w.priority = Priority::Priority;
    w.tags = vec!["MSG".into()];
    w.from_text = "Shelter-2".into();
    lb.save_new(&w, &rid(), &operator()).unwrap();
    let q = |text: &str| lb.list_entries(&EntryQuery { text: text.into(), ..Default::default() }).unwrap().total;
    assert_eq!(q("shelter"), 2);
    assert_eq!(q("capac"), 1, "prefix search");
    assert_eq!(q("\"; DROP TABLE entries; --"), 0, "hostile input is just a search term");
    lb.save_changes(&a.id, 1, &input("Generator refuel"), "", &rid(), &operator()).unwrap();
    assert_eq!(lb.list_entries(&EntryQuery { text: "capacity".into(), ..Default::default() }).unwrap().total, 0);
    assert_eq!(lb.list_entries(&EntryQuery { text: "generator".into(), ..Default::default() }).unwrap().total, 1);
    let p = lb.list_entries(&EntryQuery { priorities: vec![Priority::Priority], ..Default::default() }).unwrap();
    assert_eq!(p.items[0].subject, "Welfare check");
    assert_eq!(lb.list_entries(&EntryQuery { tags: vec!["msg".into()], ..Default::default() }).unwrap().total, 1);
    assert_eq!(lb.list_entries(&EntryQuery { station: "shelter-2".into(), ..Default::default() }).unwrap().total, 1);
    lb.set_favorite(&a.id, true).unwrap();
    assert_eq!(lb.list_entries(&EntryQuery { favorites_only: true, ..Default::default() }).unwrap().total, 1);
}

#[test]
fn templates_never_carry_timestamps_and_name_unique() {
    let parent = tempfile::tempdir().unwrap();
    let dir = create_book(parent.path(), LogbookType::Ham, "Templates");
    let mut lb = Logbook::open(&dir).unwrap();
    let mut d = TemplateDefaults { kind: Some(EntryKind::Qso), subject: "POTA".into(), ..Default::default() };
    d.ham = Some(HamFields { qso_start_utc: Some("2026-11-02T10:00:00Z".into()), mode: "ssb".into(), ..Default::default() });
    let t = lb.save_template(None, "POTA hunt", &d).unwrap();
    assert!(t.defaults.ham.as_ref().unwrap().qso_start_utc.is_none());
    assert_eq!(t.defaults.ham.as_ref().unwrap().mode, "SSB");
    assert!(lb.save_template(None, "pota HUNT", &d).is_err());
    lb.delete_template(&t.id).unwrap();
    assert!(lb.templates().unwrap().is_empty());
}

#[test]
fn creation_is_atomic_and_rejects_collisions() {
    let parent = tempfile::tempdir().unwrap();
    create_book(parent.path(), LogbookType::General, "Same Name");
    let e = emcomm_core::logbook::Logbook::create(emcomm_core::logbook::CreateParams {
        logbook_type: LogbookType::General,
        name: "Same Name".into(),
        parent_dir: parent.path().to_path_buf(),
        incident: Default::default(),
        profile: operator(),
        preset: None,
    })
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::AlreadyExists);
    let leftovers: Vec<_> = std::fs::read_dir(parent.path()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with('.')).collect();
    assert!(leftovers.is_empty());
    // Emergency requires an incident name.
    let e = emcomm_core::logbook::Logbook::create(emcomm_core::logbook::CreateParams {
        logbook_type: LogbookType::Emergency,
        name: "No incident".into(),
        parent_dir: parent.path().to_path_buf(),
        incident: Default::default(),
        profile: operator(),
        preset: None,
    })
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::Validation);
}

#[test]
fn descriptor_is_reconciled_from_database() {
    let parent = tempfile::tempdir().unwrap();
    let dir = create_book(parent.path(), LogbookType::General, "Desc");
    std::fs::remove_file(dir.join("logbook.json")).unwrap();
    let lb = Logbook::open(&dir).unwrap();
    assert!(lb.notices.iter().any(|n| n.contains("recreated")));
    assert!(dir.join("logbook.json").exists());
}

#[test]
fn app_index_tracks_logbooks_and_preset_usage() {
    let app = tempfile::tempdir().unwrap();
    let parent = tempfile::tempdir().unwrap();
    let mut db = AppDb::open(app.path()).unwrap();
    let preset = db.list_presets().unwrap().into_iter().find(|p| p.logbook_type == LogbookType::General).unwrap();
    let dir = create_book_with_preset(parent.path(), LogbookType::General, "Indexed", Some(preset.clone()));
    let lb = Logbook::open(&dir).unwrap();
    db.register_logbook(&lb.info().unwrap(), &lb.stats(None, None).unwrap(), Some(&preset.id), None).unwrap();
    assert!(db.get_preset(&preset.id).unwrap().in_use);
    assert!(db.delete_preset(&preset.id).is_err());
    let rows = db.list_index().unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].available);
}
