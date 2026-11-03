//! Backups, restore, and portable packages (A08, A09, A10, A13).

mod common;

use common::*;
use emcomm_core::backup::{self, list_backups, preview_backup, restore_backup, run_backup};
use emcomm_core::error::ErrorCode;
use emcomm_core::faults::{self, Fault};
use emcomm_core::logbook::Logbook;
use emcomm_core::models::*;
use emcomm_core::package::{export_package, import_package, preview_package};
use emcomm_core::snapshot::{self, live_data_marker, BACKUP_FORMAT};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

fn visible_dirs(p: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(p).unwrap().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    v.sort();
    v
}

fn hidden_leftovers(p: &Path) -> Vec<String> {
    fs::read_dir(p).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.starts_with('.')).collect()
}

/// A logbook with edits, a void, a favorite, a template, notes, and a Unicode-named attachment.
fn rich_book(parent: &Path, ty: LogbookType, name: &str) -> (PathBuf, Vec<Entry>) {
    let dir = create_book(parent, ty, name);
    let mut lb = Logbook::open(&dir).unwrap();
    let src = parent.join("Fotó – 現場.jpg");
    fs::write(&src, "pretend jpeg ✓".repeat(50)).unwrap();
    let staged = lb.stage_attachment(&src).unwrap();
    let mut first = if ty == LogbookType::Ham { qso("W1AW", "2026-11-02T19:00:00Z", Some(14_074_000), "20m", "FT8") } else { input("Shelter capacity 68/100") };
    if ty == LogbookType::Emergency {
        first.kind = EntryKind::Message;
        first.handling = Some(Handling::AwaitingAck);
        first.emergency = Some(EmergencyFields {
            message_number: "MSG-0042".into(),
            precedence: "PRIORITY".into(),
            originator: "Shelter-1 Manager".into(),
            originator_position: "Shelter Manager".into(),
            recipient: "EOC Logistics".into(),
            recipient_position: "Logistics Chief".into(),
            sender: "KJ4ABC".into(),
            receiver: "KJ4MRC".into(),
            sent_at_utc: Some("2026-11-02T19:10:00Z".into()),
            reply_requested: true,
            action_requested: "Send cots (20), blankets (40), water (case)".into(),
            ..Default::default()
        });
    }
    first.new_attachments = vec![staged];
    let a = lb.save_new(&first, &rid(), &operator()).unwrap().entry;
    let mut second = input("Road closure CR-214");
    second.kind = if ty == LogbookType::Emergency { EntryKind::Activity } else { EntryKind::Traffic };
    if ty == LogbookType::Emergency {
        second.emergency = Some(EmergencyFields {
            resource_unit: "Comm Unit".into(),
            assignment: "Net control".into(),
            activity_start_utc: Some("2026-11-02T18:00:00Z".into()),
            activity_end_utc: Some("2026-11-02T20:00:00Z".into()),
            disposition: "Ongoing".into(),
            preparer: "KJ4MRC".into(),
            ..Default::default()
        });
    }
    let b = lb.save_new(&second, &rid(), &operator()).unwrap().entry;
    let c = lb.save_new(&input("Wrong frequency"), &rid(), &operator()).unwrap().entry;
    let mut edited = first.clone();
    edited.new_attachments.clear();
    edited.subject = format!("{} (edited)", edited.subject);
    lb.save_changes(&a.id, 1, &edited, "update", &rid(), &operator()).unwrap();
    lb.void_entry(&c.id, "logged on wrong net", &rid(), &operator()).unwrap();
    lb.set_favorite(&b.id, true).unwrap();
    lb.save_template(None, "Ünïcödé template", &TemplateDefaults { subject: "Template subject".into(), ..Default::default() }).unwrap();
    lb.update_settings(Some("utc".into()), "OP03 active 1200-2000 local".into()).unwrap();
    let all = lb.list_entries(&all_query()).unwrap().items;
    (dir, all)
}

// ---------- A08 ----------

#[test]
fn a08_backups_are_independent_verified_and_retained() {
    let parent = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let (dir, entries) = rich_book(parent.path(), LogbookType::General, "Backed Up");
    let keep_all = BackupSettings { keep_recent: 50, keep_daily: 7, keep_weekly: 4, ..Default::default() };
    let m = run_backup(&dir, dest.path(), "manual", &keep_all).unwrap();
    let a = run_backup(&dir, dest.path(), "auto", &keep_all).unwrap();
    let c = run_backup(&dir, dest.path(), "close", &keep_all).unwrap();
    assert!(m.path != a.path && a.path != c.path);
    for r in [&m, &a, &c] {
        let man = snapshot::verify_dir(Path::new(&r.path), BACKUP_FORMAT).unwrap();
        assert_eq!(man.entry_count, entries.len() as i64);
        assert_eq!(man.files.len(), 2, "db + one attachment");
    }
    assert_eq!(list_backups(dest.path()).unwrap().len(), 3);
    // Independent copy: changing the live book does not change the backup.
    {
        let mut lb = Logbook::open(&dir).unwrap();
        lb.save_new(&input("after backup"), &rid(), &operator()).unwrap();
    }
    assert_eq!(preview_backup(Path::new(&m.path)).unwrap().entry_count, entries.len() as i64);
    assert_ne!(live_data_marker(&dir).unwrap(), m.data_marker);
    // A failed backup is never published and leaves earlier backups intact.
    faults::inject("backup.before_verify", Fault::Io, 1);
    assert!(run_backup(&dir, dest.path(), "auto", &keep_all).is_err());
    faults::clear();
    assert_eq!(list_backups(dest.path()).unwrap().len(), 3);
    let book_dir = Path::new(&m.path).parent().unwrap();
    assert!(hidden_leftovers(book_dir).is_empty(), "temp dir removed after failure");
    // A corrupted backup fails verification.
    let att = fs::read_dir(Path::new(&a.path).join("attachments")).unwrap().next().unwrap().unwrap().path();
    fs::write(&att, b"tampered").unwrap();
    assert_eq!(preview_backup(Path::new(&a.path)).unwrap_err().code, ErrorCode::Integrity);
    // Retention after a new success keeps only the newest N.
    let tight = BackupSettings { keep_recent: 2, keep_daily: 0, keep_weekly: 0, ..Default::default() };
    let latest = run_backup(&dir, dest.path(), "manual", &tight).unwrap();
    assert!(latest.pruned >= 2);
    let left = list_backups(dest.path()).unwrap();
    assert_eq!(left.len(), 2);
    assert_eq!(left[0].path, latest.path);
    // Destination inside the live logbook is refused.
    let inside = dir.join("attachments");
    assert_eq!(run_backup(&dir, &inside, "manual", &keep_all).unwrap_err().code, ErrorCode::Validation);
}

#[test]
fn a08_backup_fails_cleanly_when_destination_missing() {
    let parent = tempfile::tempdir().unwrap();
    let (dir, _) = rich_book(parent.path(), LogbookType::General, "No Dest");
    let e = run_backup(&dir, &parent.path().join("unplugged-usb"), "auto", &BackupSettings::default()).unwrap_err();
    assert_eq!(e.code, ErrorCode::StorageUnavailable);
}

// ---------- A09 ----------

#[test]
fn a09_restore_creates_distinct_book_with_entries_and_attachments() {
    let parent = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let (dir, entries) = rich_book(parent.path(), LogbookType::Emergency, "Restore Me");
    let original_id = Logbook::open(&dir).unwrap().id.clone();
    let b = run_backup(&dir, dest.path(), "manual", &BackupSettings::default()).unwrap();
    let before = visible_dirs(target.path());
    let out = restore_backup(Path::new(&b.path), target.path(), Some("Restore Me (restored)"), &|id| id == original_id).unwrap();
    assert!(out.new_identity);
    assert_ne!(out.logbook_id, original_id);
    assert_eq!(out.source_uuid, original_id);
    assert_eq!(visible_dirs(target.path()).len(), before.len() + 1);
    let mut lb = Logbook::open(&out.path).unwrap();
    let info = lb.info().unwrap();
    assert_eq!(info.name, "Restore Me (restored)");
    assert_eq!(info.provenance[0]["action"], "restore");
    assert_eq!(info.provenance[0]["source_uuid"], original_id.as_str());
    let restored = lb.list_entries(&all_query()).unwrap().items;
    assert_eq!(restored.len(), entries.len());
    for (x, y) in restored.iter().zip(entries.iter()) {
        assert_eq!(x.id, y.id);
        assert_eq!(x.sequence_no, y.sequence_no);
        assert_eq!(x.current_revision, y.current_revision);
        assert_eq!(x.emergency, y.emergency);
        assert_eq!(x.attachments, y.attachments);
        for att in &x.attachments {
            let (h, _) = emcomm_core::fsutil::sha256_file(&out.path.join(&att.rel_path)).unwrap();
            assert_eq!(h, att.sha256);
        }
    }
    // The restored book continues numbering without reuse.
    assert_eq!(lb.save_new(&input("post-restore"), &rid(), &operator()).unwrap().entry.sequence_no, 4);
    // Source untouched.
    assert_eq!(Logbook::open(&dir).unwrap().list_entries(&all_query()).unwrap().total, entries.len() as i64);
}

#[test]
fn a09_failed_validation_leaves_existing_books_untouched() {
    let parent = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let (dir, _) = rich_book(parent.path(), LogbookType::General, "Bad Backup");
    let b = run_backup(&dir, dest.path(), "manual", &BackupSettings::default()).unwrap();
    let existing = create_book(target.path(), LogbookType::General, "Existing");
    let snapshot_before = visible_dirs(target.path());
    let mut bytes = fs::read(Path::new(&b.path).join("logbook.sqlite")).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xFF;
    fs::write(Path::new(&b.path).join("logbook.sqlite"), bytes).unwrap();
    let e = restore_backup(Path::new(&b.path), target.path(), None, &|_| false).unwrap_err();
    assert_eq!(e.code, ErrorCode::Integrity);
    assert_eq!(visible_dirs(target.path()), snapshot_before);
    assert!(hidden_leftovers(target.path()).is_empty());
    assert!(Logbook::open(&existing).is_ok());
}

#[test]
fn a09_restore_name_collision_is_refused_without_leftovers() {
    let parent = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let (dir, _) = rich_book(parent.path(), LogbookType::General, "Collide");
    let b = run_backup(&dir, dest.path(), "manual", &BackupSettings::default()).unwrap();
    let e = restore_backup(Path::new(&b.path), parent.path(), None, &|_| true).unwrap_err();
    assert_eq!(e.code, ErrorCode::AlreadyExists);
    assert!(hidden_leftovers(parent.path()).is_empty());
}

// ---------- A10 / A13 ----------

fn snapshot_all(dir: &Path) -> (Vec<Entry>, Vec<Vec<EntryRevision>>, Vec<EntryTemplate>, LogbookSettings) {
    let lb = Logbook::open(dir).unwrap();
    let entries = lb.list_entries(&all_query()).unwrap().items;
    let revs = entries.iter().map(|e| lb.revisions(&e.id).unwrap()).collect();
    (entries, revs, lb.templates().unwrap(), lb.settings().unwrap())
}

#[test]
fn a10_a13_package_round_trip_preserves_everything() {
    for ty in [LogbookType::General, LogbookType::Ham, LogbookType::Emergency] {
        let parent = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let name = format!("Тест 日本 ✓ {}", ty.as_str());
        let (dir, _) = rich_book(parent.path(), ty, &name);
        let marker_before = live_data_marker(&dir).unwrap();
        let pkg = parent.path().join("transfer ünïcödé.emlog");
        let res = export_package(&dir, &pkg).unwrap();
        assert_eq!(live_data_marker(&dir).unwrap(), marker_before, "export must not mutate the live logbook");
        assert_eq!(res.attachment_count, 1);
        let pv = preview_package(&pkg).unwrap();
        assert_eq!(pv.name, name);
        let src_id = Logbook::open(&dir).unwrap().id.clone();
        let imp = import_package(&pkg, other.path(), None, &[]).unwrap();
        assert_ne!(imp.logbook_id, src_id, "import always creates a new local identity");
        assert_eq!(imp.source_uuid, src_id);
        let (e1, r1, t1, s1) = snapshot_all(&dir);
        let (e2, r2, t2, s2) = snapshot_all(Path::new(&imp.path));
        assert_eq!(e1, e2, "entries, favorites, attachments, emergency/HAM fields");
        assert_eq!(r1, r2, "revision history");
        assert_eq!(t1, t2, "templates");
        assert_eq!(s1, s2, "logbook-specific settings");
        let lb = Logbook::open(Path::new(&imp.path)).unwrap();
        let info = lb.info().unwrap();
        assert_eq!(info.provenance.as_array().unwrap().last().unwrap()["action"], "import");
        for e in &e2 {
            for a in &e.attachments {
                assert!(Path::new(&imp.path).join(&a.rel_path).is_file());
            }
        }
        if ty == LogbookType::Emergency {
            let msg = e2.iter().find(|e| e.kind == EntryKind::Message).unwrap();
            let m = msg.emergency.as_ref().unwrap();
            assert_eq!(m.message_number, "MSG-0042");
            assert_eq!(m.incident_name, "Lake County Storm Response", "incident snapshot captured at save");
            assert_eq!(m.operational_period, "OP03 1200-2000");
            assert!(m.reply_requested);
            let act = e2.iter().find(|e| e.kind == EntryKind::Activity).unwrap();
            assert_eq!(act.emergency.as_ref().unwrap().activity_end_utc.as_deref(), Some("2026-11-02T20:00:00.000Z"));
        }
        // Importing the same package again is reported as a duplicate and never overwrites.
        let known = vec![(imp.logbook_id.clone(), Some(src_id.clone()), name.clone())];
        let again = import_package(&pkg, other.path(), Some("Second copy"), &known).unwrap();
        assert_eq!(again.duplicate_of, vec![name.clone()]);
        assert_ne!(again.logbook_id, imp.logbook_id);
        assert_eq!(visible_dirs(other.path()).len(), 2);
    }
}

fn read_zip(path: &Path) -> Vec<(String, Vec<u8>)> {
    let mut a = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    (0..a.len())
        .map(|i| {
            let mut f = a.by_index(i).unwrap();
            let mut b = Vec::new();
            f.read_to_end(&mut b).unwrap();
            (f.name().unwrap().to_string(), b)
        })
        .collect()
}

fn write_zip(path: &Path, files: &[(String, Vec<u8>)]) {
    let mut z = zip::ZipWriter::new(fs::File::create(path).unwrap());
    let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (n, b) in files {
        z.start_file(n.as_str(), o).unwrap();
        z.write_all(b).unwrap();
    }
    z.finish().unwrap();
}

#[test]
fn a10_malformed_packages_are_rejected_without_leftovers() {
    let parent = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let (dir, _) = rich_book(parent.path(), LogbookType::General, "Source");
    let good = parent.path().join("good.emlog");
    export_package(&dir, &good).unwrap();
    let files = read_zip(&good);
    let manifest: serde_json::Value = serde_json::from_slice(&files.iter().find(|(n, _)| n == "manifest.json").unwrap().1).unwrap();
    let with_manifest = |m: &serde_json::Value| -> Vec<(String, Vec<u8>)> {
        files.iter().map(|(n, b)| if n == "manifest.json" { (n.clone(), serde_json::to_vec(m).unwrap()) } else { (n.clone(), b.clone()) }).collect()
    };
    let mut cases: Vec<(&str, Vec<(String, Vec<u8>)>, ErrorCode)> = Vec::new();
    let mut traversal = files.clone();
    traversal.push(("../evil.txt".into(), b"x".to_vec()));
    cases.push(("traversal", traversal, ErrorCode::InvalidPackage));
    let mut abs = files.clone();
    abs.push(("/tmp/evil".into(), b"x".to_vec()));
    cases.push(("absolute", abs, ErrorCode::InvalidPackage));
    let mut backslash = files.clone();
    backslash.push(("attachments\\..\\evil".into(), b"x".to_vec()));
    cases.push(("backslash", backslash, ErrorCode::InvalidPackage));
    let mut extra = files.clone();
    extra.push(("notes.txt".into(), b"x".to_vec()));
    cases.push(("unexpected file", extra, ErrorCode::InvalidPackage));
    let mut wal = files.clone();
    wal.push(("logbook.sqlite-wal".into(), b"x".to_vec()));
    cases.push(("wal sidecar", wal, ErrorCode::InvalidPackage));
    cases.push(("no manifest", files.iter().filter(|(n, _)| n != "manifest.json").cloned().collect(), ErrorCode::InvalidPackage));
    let mut m = manifest.clone();
    m["format_version"] = serde_json::json!(99);
    cases.push(("future version", with_manifest(&m), ErrorCode::UnsupportedVersion));
    let mut m = manifest.clone();
    m["schema_version"] = serde_json::json!(99);
    cases.push(("future schema", with_manifest(&m), ErrorCode::UnsupportedVersion));
    let mut m = manifest.clone();
    m["format"] = serde_json::json!("SOMETHING-ELSE");
    cases.push(("wrong format", with_manifest(&m), ErrorCode::InvalidPackage));
    let mut m = manifest.clone();
    m["files"][0]["sha256"] = serde_json::json!("0".repeat(64));
    cases.push(("hash mismatch", with_manifest(&m), ErrorCode::Integrity));
    cases.push(("malformed manifest", files.iter().map(|(n, b)| if n == "manifest.json" { (n.clone(), b"{not json".to_vec()) } else { (n.clone(), b.clone()) }).collect(), ErrorCode::InvalidPackage));
    let mut not_sqlite = files.clone();
    for (n, b) in not_sqlite.iter_mut() {
        if n == "logbook.sqlite" {
            *b = b"this is not a database".to_vec();
        }
    }
    let mut m = manifest.clone();
    for f in m["files"].as_array_mut().unwrap() {
        if f["path"] == "logbook.sqlite" {
            f["bytes"] = serde_json::json!(22);
            f["sha256"] = serde_json::json!(emcomm_core::fsutil::sha256_bytes(b"this is not a database"));
        }
    }
    let not_sqlite: Vec<_> = not_sqlite.into_iter().map(|(n, b)| if n == "manifest.json" { (n, serde_json::to_vec(&m).unwrap()) } else { (n, b) }).collect();
    cases.push(("invalid sqlite", not_sqlite, ErrorCode::InvalidPackage));

    for (label, contents, code) in cases {
        let p = parent.path().join(format!("{}.emlog", label.replace(' ', "_")));
        write_zip(&p, &contents);
        let e = import_package(&p, target.path(), Some(label), &[]).unwrap_err();
        assert_eq!(e.code, code, "{label}: {e:?}");
        assert!(visible_dirs(target.path()).is_empty(), "{label}: no logbook registered");
        assert!(hidden_leftovers(target.path()).is_empty(), "{label}: temp extraction removed");
    }

    // Symbolic link member.
    let p = parent.path().join("symlink.emlog");
    {
        let mut z = zip::ZipWriter::new(fs::File::create(&p).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        for (n, b) in &files {
            z.start_file(n.as_str(), o).unwrap();
            z.write_all(b).unwrap();
        }
        z.add_symlink(format!("attachments/{}.txt", "a".repeat(64)), "/etc/passwd", o).unwrap();
        z.finish().unwrap();
    }
    assert_eq!(import_package(&p, target.path(), None, &[]).unwrap_err().code, ErrorCode::InvalidPackage);

    // Duplicate member names (patched in place so the writer cannot refuse them).
    let att_name = files.iter().find(|(n, _)| n.starts_with("attachments/")).unwrap().0.clone();
    let fake = format!("attachments/{}{}", "b".repeat(64), &att_name[att_name.len() - 4..]);
    let mut dup = files.clone();
    dup.push((fake.clone(), b"dup".to_vec()));
    let p = parent.path().join("dup.emlog");
    write_zip(&p, &dup);
    let raw = fs::read(&p).unwrap();
    let patched = String::from_utf8_lossy(&raw).len(); // ensure valid read
    assert!(patched > 0);
    let mut bytes = raw.clone();
    let (from, to) = (fake.as_bytes(), att_name.as_bytes());
    assert_eq!(from.len(), to.len());
    let mut i = 0;
    while i + from.len() <= bytes.len() {
        if &bytes[i..i + from.len()] == from {
            bytes[i..i + from.len()].copy_from_slice(to);
        }
        i += 1;
    }
    fs::write(&p, bytes).unwrap();
    let e = import_package(&p, target.path(), None, &[]).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidPackage, "{e:?}");

    // Truncated / corrupted archive bytes.
    let mut bytes = fs::read(&good).unwrap();
    bytes.truncate(bytes.len() / 2);
    let p = parent.path().join("truncated.emlog");
    fs::write(&p, bytes).unwrap();
    assert_eq!(import_package(&p, target.path(), None, &[]).unwrap_err().code, ErrorCode::InvalidPackage);
    assert!(visible_dirs(target.path()).is_empty());
    assert!(hidden_leftovers(target.path()).is_empty());
}

#[test]
fn package_export_failure_leaves_no_partial_file() {
    let parent = tempfile::tempdir().unwrap();
    let (dir, _) = rich_book(parent.path(), LogbookType::General, "Partial");
    let pkg = parent.path().join("out.emlog");
    faults::inject("package.write", Fault::DiskFull, 1);
    assert_eq!(export_package(&dir, &pkg).unwrap_err().code, ErrorCode::DiskFull);
    faults::clear();
    assert!(!pkg.exists());
    assert!(hidden_leftovers(parent.path()).is_empty());
}

#[test]
fn restore_keeps_uuid_when_not_known_locally() {
    let parent = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let (dir, _) = rich_book(parent.path(), LogbookType::Ham, "Keep Id");
    let id = Logbook::open(&dir).unwrap().id.clone();
    let b = run_backup(&dir, dest.path(), "manual", &BackupSettings::default()).unwrap();
    let out = backup::restore_backup(Path::new(&b.path), target.path(), None, &|_| false).unwrap();
    assert!(!out.new_identity);
    assert_eq!(out.logbook_id, id);
}
