//! 2.1: station vs operator, shift switching, two locations per entry, activation references,
//! archived-logbook upgrades, package round trip (DESK-OP-01..03, DESK-LOC-01, DESK-MIG-01).

mod common;

use common::*;
use emcomm_core::logbook::{DB_FILE, Logbook, PRE_MIGRATION_DIR};
use emcomm_core::migrate::{self, LOGBOOK_MIGRATIONS};
use emcomm_core::models::*;
use emcomm_core::package::{export_package, import_package};
use emcomm_core::station::snapshot_default;
use emcomm_core::validate::{normalize_pota_ref, normalize_sota_ref, validate_location};
use rusqlite::Connection;
use std::fs;
use std::path::Path;

fn person(name: &str, call: &str) -> OperatorSnapshot {
    OperatorSnapshot { profile_id: Some(emcomm_core::fsutil::new_uuid()), name: name.into(), callsign: call.into(), ..Default::default() }
}

fn gps(lat: f64, lon: f64) -> Location {
    Location {
        latitude_deg: Some(lat),
        longitude_deg: Some(lon),
        label: String::new(),
        maidenhead: String::new(),
        source: LocationSource::ExternalUsbNmea,
        provider_id: "/dev/cu.usbserial-1410 @ 4800".into(),
        captured_at_utc: Some("2026-11-02T14:00:00Z".into()),
        accuracy_m: None,
        altitude_m: Some(212.5),
        satellites: Some(9),
        notes: String::new(),
    }
}

fn place(label: &str) -> Location {
    Location { latitude_deg: None, longitude_deg: None, source: LocationSource::Reported, captured_at_utc: None, ..gps(0.0, 0.0) }
        .with_label(label)
}

trait WithLabel {
    fn with_label(self, l: &str) -> Self;
}
impl WithLabel for Location {
    fn with_label(mut self, l: &str) -> Self {
        self.label = l.into();
        self.altitude_m = None;
        self.satellites = None;
        self.provider_id = String::new();
        self
    }
}

#[test]
fn desk_op_01_02_03_station_persists_across_operator_switch_and_history_is_unchanged() {
    let t = tempfile::tempdir().unwrap();
    let dir = create_book(t.path(), LogbookType::Emergency, "Command Post");
    let mut lb = Logbook::open(&dir).unwrap();
    lb.set_station(&Station {
        label: "Command post".into(),
        tactical_id: "MARC-5 COMMS".into(),
        agency: "Lake County EM".into(),
        ..Default::default()
    })
    .unwrap();
    let op = lb
        .save_period(
            None,
            &OperationalPeriodInput {
                name: "OP 1".into(),
                start_utc: "2026-11-02T07:00:00Z".into(),
                end_utc: "2026-11-02T19:00:00Z".into(),
                notes: String::new(),
            },
        )
        .unwrap()
        .period;

    let a = person("Alex Able", "KJ4AAA");
    let b = person("Bea Baker", "");
    let who = lb.ensure_operator(&a).unwrap();
    assert_eq!(who.name, "Alex Able", "first shift starts with the selected profile");
    let e1 = lb.save_new(&input("Net opened"), &rid(), &who).unwrap().entry;
    assert_eq!(e1.station_snapshot.tactical_id, "MARC-5 COMMS");

    // Handover; validation first.
    assert_eq!(lb.switch_operator(&a, None, "").unwrap_err().field.as_deref(), Some("operator"), "already on duty");
    assert_eq!(lb.switch_operator(&b, Some("2099-01-01T00:00:00Z"), "").unwrap_err().field.as_deref(), Some("at_utc"));
    assert_eq!(lb.switch_operator(&b, Some("2001-01-01T00:00:00Z"), "").unwrap_err().field.as_deref(), Some("at_utc"));
    let s = lb.switch_operator(&b, None, "Shift change 1900").unwrap();
    assert_eq!(s.operator.name, "Bea Baker");
    let who2 = lb.ensure_operator(&a).unwrap();
    assert_eq!(who2.name, "Bea Baker", "the open shift wins over the app's default profile");
    let e2 = lb.save_new(&input("Relieved"), &rid(), &who2).unwrap().entry;

    // Same station, different people; the earlier entry is untouched.
    assert_eq!(e2.station_snapshot.tactical_id, "MARC-5 COMMS");
    assert_eq!(lb.get_entry(&e1.id).unwrap().operator_snapshot.name, "Alex Able");
    assert_eq!(lb.get_entry(&e2.id).unwrap().operator_snapshot.name, "Bea Baker");
    let sessions = lb.operator_sessions().unwrap();
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].ended_at_utc.as_deref(), Some(sessions[1].started_at_utc.as_str()), "handover is one instant");
    assert_eq!(sessions[1].note, "Shift change 1900");

    // DESK-OP-02: the operational period is not ended or changed by the switch.
    let p = lb.periods().unwrap().into_iter().find(|x| x.id == op.id).unwrap();
    assert_eq!((p.status, p.end_utc.as_str()), (EntryStatus::Active, op.end_utc.as_str()));

    // Changing the station later applies prospectively only.
    lb.set_station(&Station { tactical_id: "MARC-6 COMMS".into(), ..lb.station().unwrap() }).unwrap();
    assert_eq!(lb.get_entry(&e1.id).unwrap().station_snapshot.tactical_id, "MARC-5 COMMS");
    // An edit keeps the original station snapshot.
    let mut edit = input("Net opened (edited)");
    edit.event_at_utc = e1.event_at_utc.clone();
    lb.save_changes(&e1.id, 1, &edit, "", &rid(), &who).unwrap();
    assert_eq!(lb.get_entry(&e1.id).unwrap().station_snapshot.tactical_id, "MARC-5 COMMS");

    // Filter by operator: profile id, or snapshot name.
    let q = |op: &str| lb.list_entries(&EntryQuery { operator: op.into(), ..all_query() }).unwrap().items.len();
    assert_eq!(q(a.profile_id.as_deref().unwrap()), 1);
    assert_eq!(q("Bea Baker"), 1);
}

#[test]
fn desk_loc_01_two_locations_are_independent_and_snapshots_do_not_follow_defaults() {
    let t = tempfile::tempdir().unwrap();
    let dir = create_book(t.path(), LogbookType::Emergency, "Field");
    let mut lb = Logbook::open(&dir).unwrap();
    let home = Location { label: "EOC parking lot".into(), maidenhead: "em73wv".into(), ..gps(33.8121, -84.4011) };
    lb.set_station(&Station { default_location: Some(home.clone()), ..Default::default() }).unwrap();
    let stored_default = lb.station().unwrap().default_location.unwrap();
    assert_eq!(stored_default.maidenhead, "EM73wv", "grid normalised");

    let mut i = input("Bridge out");
    i.operator_location = Some(snapshot_default(&stored_default));
    i.reported_location = Some(place("CR-12 bridge at Pine Rd"));
    let e = lb.save_new(&i, &rid(), &operator()).unwrap().entry;
    let op_loc = e.operator_location.clone().unwrap();
    assert_eq!(op_loc.source, LocationSource::LogbookDefault);
    assert!(op_loc.provider_id.starts_with("external_usb_nmea"), "original source kept: {}", op_loc.provider_id);
    assert_eq!((op_loc.latitude_deg, op_loc.longitude_deg), (Some(33.8121), Some(-84.4011)), "full precision kept");
    let rep = e.reported_location.clone().unwrap();
    assert_eq!((rep.label.as_str(), rep.latitude_deg), ("CR-12 bridge at Pine Rd", None), "reported place is not the station position");

    // The default moves; the saved entry does not.
    lb.set_station(&Station { default_location: Some(gps(34.0, -84.0)), ..lb.station().unwrap() }).unwrap();
    assert_eq!(lb.get_entry(&e.id).unwrap().operator_location.unwrap().latitude_deg, Some(33.8121));

    // A captured fix is stored as captured (no accuracy invented).
    let mut j = input("Portable check-in");
    j.operator_location = Some(gps(33.9, -84.3));
    let e2 = lb.save_new(&j, &rid(), &operator()).unwrap().entry;
    let l2 = e2.operator_location.unwrap();
    assert_eq!((l2.source, l2.accuracy_m, l2.satellites), (LocationSource::ExternalUsbNmea, None, Some(9)));
    assert!(e2.reported_location.is_none());

    // Missing location never blocks saving.
    assert!(lb.save_new(&input("No location"), &rid(), &operator()).is_ok());
}

#[test]
fn location_and_reference_validation() {
    let bad = |l: Location| validate_location("loc", &l).unwrap_err().field.unwrap();
    assert_eq!(bad(gps(91.0, 0.0)), "loc.latitude_deg");
    assert_eq!(bad(gps(0.0, 181.0)), "loc.longitude_deg");
    assert_eq!(bad(gps(f64::NAN, 0.0)), "loc.latitude_deg");
    assert_eq!(bad(Location { longitude_deg: None, ..gps(10.0, 10.0) }), "loc.latitude_deg", "both or neither");
    assert_eq!(bad(Location { accuracy_m: Some(-1.0), ..gps(10.0, 10.0) }), "loc.accuracy_m");
    assert_eq!(bad(place("")), "loc.label", "something to locate is required");
    assert_eq!(bad(Location { maidenhead: "ZZ99".into(), ..place("x") }), "loc.maidenhead");
    assert!(validate_location("loc", &gps(-90.0, 180.0)).is_ok());

    assert_eq!(normalize_pota_ref("p", " us-0005 ").unwrap(), "US-0005");
    assert_eq!(normalize_pota_ref("p", "K-1234").unwrap(), "K-1234", "older prefixes still accepted");
    assert_eq!(normalize_pota_ref("p", "ve-5082@ca-ab").unwrap(), "VE-5082@CA-AB");
    assert_eq!(normalize_pota_ref("p", "US-12345").unwrap(), "US-12345");
    for bad in ["US0005", "US-005", "TOOLONG-1234", "US-0005@X", "US-00A5"] {
        assert!(normalize_pota_ref("p", bad).is_err(), "{bad}");
    }
    assert_eq!(normalize_sota_ref("s", "w2/we-003").unwrap(), "W2/WE-003");
    assert_eq!(normalize_sota_ref("s", "G/LD-003").unwrap(), "G/LD-003");
    for bad in ["W2WE-003", "W2/WE-03", "W2/W-003", "/WE-003"] {
        assert!(normalize_sota_ref("s", bad).is_err(), "{bad}");
    }

    // On a QSO, bad references are refused with the field named.
    let t = tempfile::tempdir().unwrap();
    let dir = create_book(t.path(), LogbookType::Ham, "Activation");
    let mut lb = Logbook::open(&dir).unwrap();
    let mut q = qso("W1AW", "2026-11-02T19:00:00Z", Some(14_074_000), "20m", "FT8");
    q.ham.as_mut().unwrap().my_pota_ref = "US0005".into();
    assert_eq!(lb.save_new(&q, &rid(), &operator()).unwrap_err().field.as_deref(), Some("ham.my_pota_ref"));
    q.ham.as_mut().unwrap().my_pota_ref = "us-0005".into();
    q.ham.as_mut().unwrap().sota_ref = "w2/we-003".into();
    let e = lb.save_new(&q, &rid(), &operator()).unwrap().entry;
    let h = e.ham.unwrap();
    assert_eq!((h.my_pota_ref.as_str(), h.sota_ref.as_str()), ("US-0005", "W2/WE-003"));
}

fn downgrade(dir: &Path, to: u32) {
    let mut c = Connection::open(dir.join(DB_FILE)).unwrap();
    migrate::rollback(&mut c, LOGBOOK_MIGRATIONS, to).unwrap();
    c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).unwrap();
}

#[test]
fn archived_older_logbook_is_viewed_from_an_upgraded_copy_and_upgraded_only_on_unarchive() {
    let t = tempfile::tempdir().unwrap();
    let dir = create_book(t.path(), LogbookType::General, "Old archive");
    {
        let mut lb = Logbook::open(&dir).unwrap();
        lb.save_new(&input("Kept forever"), &rid(), &operator()).unwrap();
        lb.set_archived(true).unwrap();
    }
    downgrade(&dir, 3);
    let before = fs::read(dir.join(DB_FILE)).unwrap();

    let lb = Logbook::open(&dir).unwrap();
    assert!(lb.read_only);
    assert!(lb.notices.iter().any(|n| n.contains("temporary copy")), "{:?}", lb.notices);
    assert_eq!(lb.list_entries(&all_query()).unwrap().items[0].subject, "Kept forever");
    assert!(lb.station().is_ok(), "v4 features readable from the view copy");
    drop(lb);
    assert_eq!(fs::read(dir.join(DB_FILE)).unwrap(), before, "archived file unchanged");
    assert!(!dir.join(PRE_MIGRATION_DIR).exists());

    let mut lb = Logbook::open(&dir).unwrap();
    lb.set_archived(false).unwrap();
    assert!(!lb.read_only);
    assert!(lb.notices.iter().any(|n| n.contains("upgraded in place")));
    lb.save_new(&input("Now writable"), &rid(), &operator()).unwrap();
    drop(lb);
    let c = Connection::open(dir.join(DB_FILE)).unwrap();
    assert_eq!(migrate::current_version(&c).unwrap(), migrate::latest_version(LOGBOOK_MIGRATIONS));
    let kept: Vec<_> = fs::read_dir(dir.join(PRE_MIGRATION_DIR)).unwrap().flatten().collect();
    assert_eq!(kept.len(), 1);
    assert_eq!(migrate::current_version(&Connection::open(kept[0].path()).unwrap()).unwrap(), 3);
}

#[test]
fn desk_mig_01_v3_logbook_upgrades_and_package_round_trips_identity_and_locations() {
    let t = tempfile::tempdir().unwrap();
    let dir = create_book(t.path(), LogbookType::Ham, "POTA trip");
    {
        let mut lb = Logbook::open(&dir).unwrap();
        lb.save_new(&qso("K1ABC", "2026-11-02T15:00:00Z", Some(7_074_000), "40m", "FT8"), &rid(), &operator()).unwrap();
    }
    downgrade(&dir, 3);
    let mut lb = Logbook::open(&dir).unwrap();
    assert_eq!(lb.info().unwrap().schema_version, migrate::latest_version(LOGBOOK_MIGRATIONS));
    let old = &lb.list_entries(&all_query()).unwrap().items[0];
    assert_eq!(old.station_snapshot, StationSnapshot::default(), "older entries have an empty station snapshot");
    assert!(old.operator_location.is_none());
    lb.set_station(&Station { station_callsign: "W4CLB".into(), pota_ref: "us-1234".into(), ..Default::default() }).unwrap();
    let a = person("Alex Able", "KJ4AAA");
    let who = lb.ensure_operator(&a).unwrap();
    let mut q = qso("N0CALL", "2026-11-02T16:00:00Z", Some(14_074_000), "20m", "FT8");
    q.operator_location = Some(gps(35.5, -83.5));
    lb.save_new(&q, &rid(), &who).unwrap();
    drop(lb);

    let pkg = t.path().join("trip.emlog");
    export_package(&dir, &pkg).unwrap();
    let other = tempfile::tempdir().unwrap();
    let imp = import_package(&pkg, other.path(), None, &[]).unwrap();
    let lb2 = Logbook::open(Path::new(&imp.path)).unwrap();
    let st = lb2.station().unwrap();
    assert_eq!((st.station_callsign.as_str(), st.pota_ref.as_str()), ("W4CLB", "US-1234"));
    assert_eq!(lb2.operator_sessions().unwrap()[0].operator.name, "Alex Able", "shift history travels; no profile needed");
    let items = lb2.list_entries(&all_query()).unwrap().items;
    let new = items.iter().find(|e| e.ham.as_ref().unwrap().their_call == "N0CALL").unwrap();
    assert_eq!(new.operator_location.as_ref().unwrap().latitude_deg, Some(35.5));
    assert_eq!(new.station_snapshot.station_callsign, "W4CLB");
}

#[test]
fn desk_qe_recents_are_local_ordered_and_skip_voided() {
    use emcomm_core::recents::RecentField::*;
    let parent = tempfile::tempdir().unwrap();
    let dir = create_book(parent.path(), LogbookType::Ham, "Recents");
    let mut lb = Logbook::open(&dir).unwrap();
    for (call, mode) in [("W1AW", "CW"), ("K1ABC", "SSB"), ("W1AW", "CW"), ("N0VOID", "FT8")] {
        let mut q = qso(call, "2026-11-02T19:00:00Z", Some(14_062_000), "20m", mode);
        q.ham.as_mut().unwrap().pota_ref = if call == "K1ABC" { "US-0005".into() } else { String::new() };
        let s = lb.save_new(&q, &rid(), &operator()).unwrap();
        if call == "N0VOID" {
            lb.void_entry(&s.entry.id, "bad", &rid(), &operator()).unwrap();
        }
    }
    let r = lb.recent_values(&[TheirCall, Mode, PotaRef, FrequencyHz, Station]).unwrap();
    assert_eq!(r[&TheirCall], vec!["W1AW", "K1ABC"], "newest first, voided excluded, distinct");
    assert_eq!(r[&Mode], vec!["CW", "SSB"]);
    assert_eq!(r[&PotaRef], vec!["US-0005"]);
    assert_eq!(r[&FrequencyHz], vec!["14062000"]);
    assert!(r[&Station].is_empty());
}
