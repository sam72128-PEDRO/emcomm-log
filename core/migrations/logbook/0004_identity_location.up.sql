-- EMCOMM LOG logbook schema v4 (2.1): station/assignment, operator shifts, two locations per entry,
-- HAM activation references. Locations are immutable JSON value objects (validated in Rust), so an
-- entry's saved location never changes when a default or receiver changes later.

-- The logbook's station / assignment (one row), edited prospectively; entries keep a snapshot.
CREATE TABLE station (
    logbook_id TEXT PRIMARY KEY NOT NULL REFERENCES logbook(id) ON UPDATE CASCADE,
    label TEXT NOT NULL DEFAULT '',
    tactical_id TEXT NOT NULL DEFAULT '',
    station_callsign TEXT NOT NULL DEFAULT '',
    agency TEXT NOT NULL DEFAULT '',
    default_location_json TEXT,
    pota_ref TEXT NOT NULL DEFAULT '',
    sota_ref TEXT NOT NULL DEFAULT '',
    updated_at_utc TEXT NOT NULL
);
INSERT INTO station(logbook_id, updated_at_utc) SELECT id, updated_at_utc FROM logbook;

-- Who was operating, and from when (sequential shifts on one station; not a login system).
CREATE TABLE operator_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    profile_id TEXT,
    operator_snapshot_json TEXT NOT NULL,
    started_at_utc TEXT NOT NULL,
    ended_at_utc TEXT CHECK (ended_at_utc IS NULL OR ended_at_utc >= started_at_utc),
    note TEXT NOT NULL DEFAULT '',
    created_at_utc TEXT NOT NULL
);
CREATE INDEX idx_operator_sessions_start ON operator_sessions(started_at_utc);

ALTER TABLE entries ADD COLUMN station_snapshot_json TEXT NOT NULL DEFAULT '{}';
ALTER TABLE entries ADD COLUMN operator_location_json TEXT;
ALTER TABLE entries ADD COLUMN reported_location_json TEXT;

ALTER TABLE entry_ham ADD COLUMN my_pota_ref TEXT NOT NULL DEFAULT '';
ALTER TABLE entry_ham ADD COLUMN pota_ref TEXT NOT NULL DEFAULT '';
ALTER TABLE entry_ham ADD COLUMN my_sota_ref TEXT NOT NULL DEFAULT '';
ALTER TABLE entry_ham ADD COLUMN sota_ref TEXT NOT NULL DEFAULT '';

UPDATE logbook SET schema_version = 4;
