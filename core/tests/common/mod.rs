#![allow(dead_code)]

use emcomm_core::appdb::AppDb;
use emcomm_core::fsutil::new_uuid;
use emcomm_core::logbook::{CreateParams, Logbook};
use emcomm_core::models::*;
use std::path::{Path, PathBuf};

pub fn operator() -> OperatorSnapshot {
    OperatorSnapshot { profile_id: Some(new_uuid()), name: "Test Operator".into(), callsign: "KJ4MRC".into(), ..Default::default() }
}

pub fn incident() -> IncidentFields {
    IncidentFields {
        incident_name: "Lake County Storm Response".into(),
        incident_number: "LC-2026-11".into(),
        operational_period: "OP03 1200-2000".into(),
        agency: "MARC-5".into(),
        location: "Lake County EOC".into(),
    }
}

pub fn create_book(parent: &Path, ty: LogbookType, name: &str) -> PathBuf {
    create_book_with_preset(parent, ty, name, None)
}

pub fn create_book_with_preset(parent: &Path, ty: LogbookType, name: &str, preset: Option<Preset>) -> PathBuf {
    Logbook::create(CreateParams {
        logbook_type: ty,
        name: name.into(),
        parent_dir: parent.to_path_buf(),
        incident: if ty == LogbookType::Emergency { incident() } else { IncidentFields::default() },
        profile: operator(),
        preset,
    })
    .expect("create logbook")
}

pub fn builtin_preset(dir: &Path, ty: LogbookType) -> (AppDb, Preset) {
    let db = AppDb::open(dir).unwrap();
    let p = db.list_presets().unwrap().into_iter().find(|p| p.logbook_type == ty).unwrap();
    (db, p)
}

pub fn input(subject: &str) -> EntryInput {
    EntryInput {
        kind: EntryKind::Traffic,
        event_at_utc: "2026-11-02T14:28:00-05:00".into(),
        event_timezone: "America/New_York".into(),
        event_utc_offset_min: -300,
        direction: Direction::Received,
        priority: Priority::Routine,
        from_text: "MARC-5".into(),
        to_text: "EOC".into(),
        subject: subject.into(),
        body: "Bridge on CR-12 blocked by downed trees; road impassable.".into(),
        channel: "VHF A (146.520 MHz)".into(),
        tags: vec!["INFO".into()],
        ..Default::default()
    }
}

pub fn qso(call: &str, at: &str, hz: Option<i64>, band: &str, mode: &str) -> EntryInput {
    EntryInput {
        kind: EntryKind::Qso,
        event_at_utc: at.into(),
        subject: format!("QSO with {call}"),
        direction: Direction::Other,
        ham: Some(HamFields {
            their_call: call.into(),
            station_call: "KJ4MRC".into(),
            qso_start_utc: Some(at.into()),
            frequency_hz: hz,
            band: band.into(),
            mode: mode.into(),
            rst_sent: "59".into(),
            rst_rcvd: "57".into(),
            their_grid: "FN31pr".into(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

pub fn rid() -> String {
    new_uuid()
}

pub fn all_query() -> EntryQuery {
    EntryQuery { status: "all".into(), limit: 500, ..Default::default() }
}
