//! Scale/performance check (spec section 10). Run with:
//!   cargo test -p emcomm-core --release --test scale -- --ignored --nocapture

mod common;

use common::*;
use emcomm_core::export::{self, ExportFormat, ExportRequest};
use emcomm_core::logbook::Logbook;
use emcomm_core::models::*;
use std::time::Instant;

#[test]
#[ignore = "slow: 100,000-entry scale test"]
fn scale_100k_entries() {
    let parent = tempfile::tempdir().unwrap();
    let dir = create_book(parent.path(), LogbookType::General, "Scale");
    let mut lb = Logbook::open(&dir).unwrap();
    // Bulk-load 100k entries through the normal save path (each its own durable commit is
    // too slow for a test fixture on fsync-heavy storage, so relax sync for the load only).
    lb.conn.execute_batch("PRAGMA synchronous=OFF").unwrap();
    let t = Instant::now();
    for i in 0..100_000 {
        let mut e = input(&format!("Entry {i} shelter status report"));
        e.body = format!("Routine traffic number {i}. Generator fuel at {}%.", i % 100);
        e.from_text = format!("STN-{}", i % 250);
        lb.save_new(&e, &rid(), &operator()).unwrap();
    }
    println!("bulk load 100k: {:?}", t.elapsed());
    lb.conn.execute_batch("PRAGMA synchronous=FULL").unwrap();
    // Durable single saves at 100k scale.
    let mut worst = std::time::Duration::ZERO;
    for i in 0..20 {
        let t = Instant::now();
        lb.save_new(&input(&format!("durable {i}")), &rid(), &operator()).unwrap();
        worst = worst.max(t.elapsed());
    }
    println!("worst durable save at 100k (synchronous=FULL): {worst:?}");
    assert!(worst.as_millis() < 2000, "save feedback target is under 2 s");
    let t = Instant::now();
    let p = lb.list_entries(&EntryQuery { text: "generator 42".into(), limit: 50, ..Default::default() }).unwrap();
    println!("FTS search: {} hits in {:?}", p.total, t.elapsed());
    assert!(p.total > 0);
    let t = Instant::now();
    let p = lb.list_entries(&EntryQuery { station: "STN-7".into(), limit: 50, offset: 500, ..Default::default() }).unwrap();
    println!("station filter page: {} total in {:?}", p.total, t.elapsed());
    let info = lb.info().unwrap();
    for f in [ExportFormat::Csv, ExportFormat::Xlsx, ExportFormat::Pdf, ExportFormat::Docx, ExportFormat::Txt] {
        let t = Instant::now();
        let r = export::run_export(
            &lb.conn,
            &info,
            &ExportRequest {
                format: f,
                scope: "all".into(),
                range_from: None,
                range_to: None,
                filter: None,
                include_voided: false,
                display_zone: "utc".into(),
            },
            &parent.path().join(format!("big.{}", f.extension())),
        )
        .unwrap();
        println!("export {f:?}: {} records, {} KB in {:?}", r.exported, r.bytes / 1024, t.elapsed());
        assert_eq!(r.exported, 100_020);
    }
}
