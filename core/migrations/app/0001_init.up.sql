-- EMCOMM LOG per-user application database v1 (never exported in .emlog)
CREATE TABLE app_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    settings_json TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL
);

CREATE TABLE profiles (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    callsign TEXT NOT NULL DEFAULT '',
    license_class TEXT NOT NULL DEFAULT '',
    organization TEXT NOT NULL DEFAULT '',
    position TEXT NOT NULL DEFAULT '',
    grid TEXT NOT NULL DEFAULT '',
    active INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL
);

CREATE TABLE presets (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    logbook_type TEXT NOT NULL CHECK (logbook_type IN ('general', 'ham', 'emergency')),
    profile_id TEXT REFERENCES profiles(id),
    version INTEGER NOT NULL DEFAULT 1,
    config_json TEXT NOT NULL,
    builtin INTEGER NOT NULL DEFAULT 0,
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL
);

CREATE TABLE logbook_index (
    id TEXT PRIMARY KEY NOT NULL,
    path TEXT NOT NULL,
    name TEXT NOT NULL,
    type TEXT NOT NULL,
    incident_name TEXT NOT NULL DEFAULT '',
    location TEXT NOT NULL DEFAULT '',
    preset_id TEXT,
    source_uuid TEXT,
    entry_count INTEGER NOT NULL DEFAULT 0,
    first_event_utc TEXT,
    last_event_utc TEXT,
    last_modified_utc TEXT,
    last_opened_utc TEXT,
    archived INTEGER NOT NULL DEFAULT 0,
    registered_at_utc TEXT NOT NULL
);

CREATE TABLE backup_runs (
    rid INTEGER PRIMARY KEY,
    logbook_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('auto', 'close', 'manual')),
    started_at_utc TEXT NOT NULL,
    finished_at_utc TEXT NOT NULL,
    ok INTEGER NOT NULL CHECK (ok IN (0, 1)),
    path TEXT,
    entry_count INTEGER,
    message TEXT NOT NULL DEFAULT '',
    data_marker TEXT
);
CREATE INDEX idx_backup_runs_logbook ON backup_runs(logbook_id, rid);
