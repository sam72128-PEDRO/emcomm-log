//! Cross-OS `.emlog` round trip used by CI: one OS exports, another imports and checks.
//!   EMCOMM_EXPORT_PACKAGE=<dir>  cargo test -p emcomm-core --test crossos -- --ignored
//!   EMCOMM_IMPORT_PACKAGE=<file> cargo test -p emcomm-core --test crossos -- --ignored

mod common;

use common::*;
use emcomm_core::logbook::Logbook;
use emcomm_core::models::*;
use emcomm_core::package::{export_package, import_package};
use std::path::{Path, PathBuf};

const NAME: &str = "Кросс OS 日本 Ünïcödé ✓";

fn build(parent: &Path) -> PathBuf {
    let dir = create_book(parent, LogbookType::Emergency, NAME);
    let mut lb = Logbook::open(&dir).unwrap();
    let src = parent.join("Фото 現場 ü.jpg");
    std::fs::write(&src, "attachment ✓ bytes".repeat(100)).unwrap();
    let staged = lb.stage_attachment(&src).unwrap();
    let mut m = input("Supply request ✓");
    m.kind = EntryKind::Message;
    m.body = "Line one\nLine two — ünïcödé 日本".into();
    m.emergency =
        Some(EmergencyFields { message_number: "42".into(), originator: "Shelter-1".into(), reply_requested: true, ..Default::default() });
    m.new_attachments = vec![staged];
    let a = lb.save_new(&m, "00000000-0000-4000-8000-000000000001", &operator()).unwrap().entry;
    let mut edit = m.clone();
    edit.new_attachments.clear();
    edit.subject = "Supply request ✓ (edited)".into();
    lb.save_changes(&a.id, 1, &edit, "edited on source OS", "00000000-0000-4000-8000-000000000002", &operator()).unwrap();
    let b = lb.save_new(&input("Second"), "00000000-0000-4000-8000-000000000003", &operator()).unwrap().entry;
    lb.void_entry(&b.id, "void ✓", "00000000-0000-4000-8000-000000000004", &operator()).unwrap();
    lb.set_favorite(&a.id, true).unwrap();
    lb.save_template(None, "Шаблон ✓", &TemplateDefaults { subject: "T".into(), ..Default::default() }).unwrap();
    lb.update_settings(Some("utc".into()), "Notes ✓ 日本".into()).unwrap();
    dir
}

#[test]
#[ignore = "CI cross-OS helper"]
fn crossos_export() {
    let Ok(out) = std::env::var("EMCOMM_EXPORT_PACKAGE") else { return };
    std::fs::create_dir_all(&out).unwrap();
    let parent = tempfile::tempdir().unwrap();
    let dir = build(parent.path());
    let r = export_package(&dir, &Path::new(&out).join(format!("crossos-{}.emlog", std::env::consts::OS))).unwrap();
    println!("exported {} ({} bytes, sha256 {})", r.path, r.bytes, r.sha256);
}

fn check(dir: &Path) {
    let lb = Logbook::open(dir).unwrap();
    let info = lb.info().unwrap();
    assert_eq!(info.name, NAME);
    let all = lb.list_entries(&all_query()).unwrap().items;
    assert_eq!(all.len(), 2);
    let a = all.iter().find(|e| e.sequence_no == 1).unwrap();
    assert_eq!(a.subject, "Supply request ✓ (edited)");
    assert_eq!(a.body, "Line one\nLine two — ünïcödé 日本");
    assert!(a.favorite);
    assert_eq!(a.current_revision, 2);
    assert_eq!(a.emergency.as_ref().unwrap().message_number, "42");
    assert_eq!(a.attachments.len(), 1);
    assert_eq!(a.attachments[0].display_name, "Фото 現場 ü.jpg");
    let (h, _) = emcomm_core::fsutil::sha256_file(&dir.join(&a.attachments[0].rel_path)).unwrap();
    assert_eq!(h, a.attachments[0].sha256);
    let revs = lb.revisions(&a.id).unwrap();
    assert_eq!(revs[1].reason, "edited on source OS");
    let b = all.iter().find(|e| e.sequence_no == 2).unwrap();
    assert_eq!(b.status, EntryStatus::Void);
    assert_eq!(b.void_reason.as_deref(), Some("void ✓"));
    assert_eq!(lb.templates().unwrap()[0].name, "Шаблон ✓");
    let s = lb.settings().unwrap();
    assert_eq!(s.operational_notes, "Notes ✓ 日本");
    assert_eq!(s.time_display_override.as_deref(), Some("utc"));
}

#[test]
#[ignore = "CI cross-OS helper"]
fn crossos_import() {
    let Ok(pkg) = std::env::var("EMCOMM_IMPORT_PACKAGE") else { return };
    let target = tempfile::tempdir().unwrap();
    let r = import_package(Path::new(&pkg), target.path(), None, &[]).unwrap();
    println!("imported on {} from {}", std::env::consts::OS, r.source_uuid);
    check(Path::new(&r.path));
}

#[test]
fn same_os_round_trip_matches_crossos_expectations() {
    let parent = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let dir = build(parent.path());
    let pkg = parent.path().join("x.emlog");
    export_package(&dir, &pkg).unwrap();
    let r = import_package(&pkg, other.path(), None, &[]).unwrap();
    check(Path::new(&r.path));
}
