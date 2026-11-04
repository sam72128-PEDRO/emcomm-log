-- Schema v5 (EMCOMM LOG 2.2): attachment captions/source/removal, Important + follow-up on entries,
-- links between entries, per-logbook station directory, ADIF import batches. Additive only: no existing
-- table is rebuilt, no entry or number is rewritten.

-- Feature A: attachments
ALTER TABLE attachments ADD COLUMN caption TEXT NOT NULL DEFAULT '';
ALTER TABLE attachments ADD COLUMN source TEXT NOT NULL DEFAULT 'file'
    CHECK (source IN ('file', 'drag_drop', 'paste', 'camera', 'photo_picker', 'document_picker', 'import'));
-- Removing an attachment is an entry change: the row and bytes stay for history, the revision is recorded.
ALTER TABLE attachments ADD COLUMN removed_in_revision INTEGER;

-- Feature C: Important + follow-up (Flag for Report = entries.flagged and Favorites stay separate)
ALTER TABLE entries ADD COLUMN important INTEGER NOT NULL DEFAULT 0 CHECK (important IN (0, 1));
ALTER TABLE entries ADD COLUMN follow_up_state TEXT NOT NULL DEFAULT 'none'
    CHECK (follow_up_state IN ('none', 'open', 'resolved'));
ALTER TABLE entries ADD COLUMN follow_up_due_utc TEXT;
ALTER TABLE entries ADD COLUMN follow_up_note TEXT NOT NULL DEFAULT '';
ALTER TABLE entries ADD COLUMN follow_up_resolved_utc TEXT;
CREATE INDEX idx_entries_follow_up ON entries(follow_up_state);
CREATE INDEX idx_entries_important ON entries(important);

-- Feature B: related communications. Rows are never deleted: removal sets removed_* (audit trail).
CREATE TABLE entry_links (
    id TEXT PRIMARY KEY NOT NULL,
    source_entry_id TEXT NOT NULL REFERENCES entries(id),
    target_entry_id TEXT NOT NULL REFERENCES entries(id),
    relation TEXT NOT NULL CHECK (relation IN ('related_to', 'follow_up_to', 'relay_of', 'response_to', 'corrects')),
    note TEXT NOT NULL DEFAULT '',
    created_at_utc TEXT NOT NULL,
    created_by_json TEXT NOT NULL,
    removed_at_utc TEXT,
    removed_by_json TEXT,
    CHECK (source_entry_id <> target_entry_id)
);
CREATE UNIQUE INDEX ux_entry_links_active ON entry_links(source_entry_id, target_entry_id, relation)
    WHERE removed_at_utc IS NULL;
CREATE INDEX idx_entry_links_target ON entry_links(target_entry_id);
CREATE TRIGGER entry_links_no_delete BEFORE DELETE ON entry_links
BEGIN SELECT RAISE(ABORT, 'entry links are never deleted'); END;
CREATE TRIGGER entry_links_immutable BEFORE UPDATE ON entry_links
WHEN NEW.id <> OLD.id OR NEW.source_entry_id <> OLD.source_entry_id OR NEW.target_entry_id <> OLD.target_entry_id
  OR NEW.relation <> OLD.relation OR NEW.created_at_utc <> OLD.created_at_utc OR OLD.removed_at_utc IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'entry links are immutable once created or removed'); END;

-- Feature D: per-logbook station directory (not operator profiles, not the logbook's own station)
CREATE TABLE station_directory (
    id TEXT PRIMARY KEY NOT NULL,
    label TEXT NOT NULL CHECK (length(trim(label)) > 0),
    kind TEXT NOT NULL DEFAULT 'other'
        CHECK (kind IN ('amateur_callsign', 'tactical_identifier', 'station', 'organization', 'other')),
    identifier TEXT NOT NULL DEFAULT '',
    identifier_norm TEXT NOT NULL DEFAULT '',
    agency TEXT NOT NULL DEFAULT '',
    channel TEXT NOT NULL DEFAULT '',
    frequency_hz INTEGER CHECK (frequency_hz IS NULL OR frequency_hz > 0),
    mode TEXT NOT NULL DEFAULT '',
    aliases_json TEXT NOT NULL DEFAULT '[]',
    tags_json TEXT NOT NULL DEFAULT '[]',
    notes TEXT NOT NULL DEFAULT '',
    last_used_utc TEXT,
    last_entry_id TEXT,
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL
);
CREATE INDEX idx_directory_norm ON station_directory(identifier_norm);
CREATE INDEX idx_directory_label ON station_directory(label COLLATE NOCASE);

-- Feature E: ADIF import provenance
CREATE TABLE adif_import_batches (
    id TEXT PRIMARY KEY NOT NULL,
    source_name TEXT NOT NULL,
    source_sha256 TEXT NOT NULL CHECK (length(source_sha256) = 64),
    imported_at_utc TEXT NOT NULL,
    candidates INTEGER NOT NULL,
    imported INTEGER NOT NULL,
    skipped INTEGER NOT NULL,
    summary_json TEXT NOT NULL DEFAULT '{}',
    operator_json TEXT NOT NULL
);
CREATE TABLE entry_import_source (
    entry_id TEXT PRIMARY KEY NOT NULL REFERENCES entries(id),
    batch_id TEXT NOT NULL REFERENCES adif_import_batches(id),
    source_index INTEGER NOT NULL,
    record_sha256 TEXT NOT NULL,
    extras_json TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX idx_import_source_batch ON entry_import_source(batch_id);
CREATE INDEX idx_import_source_hash ON entry_import_source(record_sha256);

UPDATE logbook SET schema_version = 5;
