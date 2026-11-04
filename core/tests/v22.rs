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
