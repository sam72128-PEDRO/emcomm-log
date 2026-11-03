-- EMCOMM LOG logbook schema v1
CREATE TABLE logbook (
    id TEXT PRIMARY KEY NOT NULL,
    schema_version INTEGER NOT NULL,
    type TEXT NOT NULL CHECK (type IN ('general', 'ham', 'emergency')),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    incident_name TEXT NOT NULL DEFAULT '',
    incident_number TEXT NOT NULL DEFAULT '',
    operational_period TEXT NOT NULL DEFAULT '',
    agency TEXT NOT NULL DEFAULT '',
    location TEXT NOT NULL DEFAULT '',
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL,
    archived_at_utc TEXT,
    next_sequence INTEGER NOT NULL DEFAULT 1 CHECK (next_sequence >= 1),
    origin_profile_snapshot_json TEXT NOT NULL,
    provenance_json TEXT NOT NULL DEFAULT '[]',
    created_by_app_version TEXT NOT NULL
);

CREATE TABLE logbook_settings (
    logbook_id TEXT PRIMARY KEY NOT NULL REFERENCES logbook(id) ON UPDATE CASCADE ON DELETE CASCADE,
    time_display_override TEXT CHECK (time_display_override IN ('local', 'utc')),
    preset_id TEXT,
    preset_name TEXT NOT NULL DEFAULT '',
    preset_version INTEGER NOT NULL DEFAULT 0,
    preset_snapshot_json TEXT NOT NULL DEFAULT '{}',
    operational_notes TEXT NOT NULL DEFAULT '',
    updated_at_utc TEXT NOT NULL
);

CREATE TABLE entries (
    rid INTEGER PRIMARY KEY,
    id TEXT NOT NULL UNIQUE,
    logbook_id TEXT NOT NULL REFERENCES logbook(id) ON UPDATE CASCADE,
    sequence_no INTEGER NOT NULL CHECK (sequence_no >= 1),
    kind TEXT NOT NULL CHECK (kind IN ('traffic', 'message', 'status', 'note', 'qso', 'activity')),
    event_at_utc TEXT NOT NULL,
    event_timezone TEXT NOT NULL,
    event_utc_offset_min INTEGER NOT NULL,
    recorded_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'void')),
    voided_at_utc TEXT,
    void_reason TEXT,
    direction TEXT NOT NULL CHECK (direction IN ('received', 'sent', 'internal', 'other')),
    priority TEXT NOT NULL CHECK (priority IN ('routine', 'priority', 'emergency')),
    handling TEXT CHECK (handling IN ('pending', 'sent', 'awaiting_ack', 'acknowledged', 'relayed', 'delivered', 'closed')),
    flagged INTEGER NOT NULL DEFAULT 0 CHECK (flagged IN (0, 1)),
    from_text TEXT NOT NULL DEFAULT '',
    to_text TEXT NOT NULL DEFAULT '',
    subject TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    channel TEXT NOT NULL DEFAULT '',
    location TEXT NOT NULL DEFAULT '',
    operator_snapshot_json TEXT NOT NULL,
    tags_json TEXT NOT NULL DEFAULT '[]',
    current_revision INTEGER NOT NULL DEFAULT 1 CHECK (current_revision >= 1),
    client_request_id TEXT NOT NULL UNIQUE,
    UNIQUE (logbook_id, sequence_no)
);

CREATE INDEX idx_entries_event ON entries(event_at_utc, sequence_no);
CREATE INDEX idx_entries_recorded ON entries(recorded_at_utc, sequence_no);
CREATE INDEX idx_entries_status ON entries(status);
CREATE INDEX idx_entries_kind ON entries(kind);
CREATE INDEX idx_entries_handling ON entries(handling);
CREATE INDEX idx_entries_flagged ON entries(flagged);

CREATE TABLE entry_ham (
    entry_id TEXT PRIMARY KEY NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    their_call TEXT NOT NULL DEFAULT '',
    station_call TEXT NOT NULL DEFAULT '',
    operator_call TEXT NOT NULL DEFAULT '',
    qso_start_utc TEXT,
    qso_end_utc TEXT,
    frequency_hz INTEGER CHECK (frequency_hz IS NULL OR frequency_hz > 0),
    band TEXT NOT NULL DEFAULT '',
    mode TEXT NOT NULL DEFAULT '',
    submode TEXT NOT NULL DEFAULT '',
    rst_sent TEXT NOT NULL DEFAULT '',
    rst_rcvd TEXT NOT NULL DEFAULT '',
    my_grid TEXT NOT NULL DEFAULT '',
    their_grid TEXT NOT NULL DEFAULT '',
    qso_notes TEXT NOT NULL DEFAULT ''
);
CREATE INDEX idx_ham_call ON entry_ham(their_call);

CREATE TABLE entry_emergency (
    entry_id TEXT PRIMARY KEY NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    record_type TEXT NOT NULL CHECK (record_type IN ('message', 'activity', 'general')),
    incident_name TEXT NOT NULL DEFAULT '',
    incident_number TEXT NOT NULL DEFAULT '',
    operational_period TEXT NOT NULL DEFAULT '',
    message_number TEXT NOT NULL DEFAULT '',
    precedence TEXT NOT NULL DEFAULT '',
    originator TEXT NOT NULL DEFAULT '',
    originator_position TEXT NOT NULL DEFAULT '',
    recipient TEXT NOT NULL DEFAULT '',
    recipient_position TEXT NOT NULL DEFAULT '',
    sender TEXT NOT NULL DEFAULT '',
    receiver TEXT NOT NULL DEFAULT '',
    sent_at_utc TEXT,
    received_at_utc TEXT,
    reply_requested INTEGER NOT NULL DEFAULT 0 CHECK (reply_requested IN (0, 1)),
    action_requested TEXT NOT NULL DEFAULT '',
    response TEXT NOT NULL DEFAULT '',
    resource_unit TEXT NOT NULL DEFAULT '',
    assignment TEXT NOT NULL DEFAULT '',
    activity_start_utc TEXT,
    activity_end_utc TEXT,
    location TEXT NOT NULL DEFAULT '',
    disposition TEXT NOT NULL DEFAULT '',
    preparer TEXT NOT NULL DEFAULT ''
);
CREATE INDEX idx_emergency_incident ON entry_emergency(incident_name);

CREATE TABLE entry_revisions (
    rid INTEGER PRIMARY KEY,
    entry_id TEXT NOT NULL REFERENCES entries(id),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    action TEXT NOT NULL CHECK (action IN ('create', 'edit', 'void', 'restore')),
    at_utc TEXT NOT NULL,
    reason TEXT NOT NULL DEFAULT '',
    before_json TEXT,
    after_json TEXT NOT NULL,
    editor_snapshot_json TEXT NOT NULL,
    client_request_id TEXT NOT NULL UNIQUE,
    UNIQUE (entry_id, revision)
);

CREATE TRIGGER entry_revisions_immutable_update BEFORE UPDATE ON entry_revisions
BEGIN
    SELECT RAISE(ABORT, 'entry revisions are immutable');
END;
CREATE TRIGGER entry_revisions_immutable_delete BEFORE DELETE ON entry_revisions
BEGIN
    SELECT RAISE(ABORT, 'entry revisions are immutable');
END;

CREATE TABLE attachments (
    id TEXT PRIMARY KEY NOT NULL,
    entry_id TEXT NOT NULL REFERENCES entries(id),
    rel_path TEXT NOT NULL,
    display_name TEXT NOT NULL,
    mime TEXT NOT NULL DEFAULT 'application/octet-stream',
    size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
    added_at_utc TEXT NOT NULL,
    added_in_revision INTEGER NOT NULL
);
CREATE INDEX idx_attachments_entry ON attachments(entry_id);
CREATE INDEX idx_attachments_path ON attachments(rel_path);

CREATE TABLE entry_templates (
    id TEXT PRIMARY KEY NOT NULL,
    logbook_id TEXT NOT NULL REFERENCES logbook(id) ON UPDATE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    kind TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    defaults_json TEXT NOT NULL,
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL,
    UNIQUE (logbook_id, name)
);

CREATE TABLE entry_favorites (
    entry_id TEXT PRIMARY KEY NOT NULL REFERENCES entries(id),
    created_at_utc TEXT NOT NULL
);

CREATE VIRTUAL TABLE entries_fts USING fts5(
    subject, body, parties, tags, extra,
    tokenize = 'unicode61 remove_diacritics 2'
);
