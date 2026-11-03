-- EMCOMM LOG logbook schema v2: operational periods, resources assigned, ICS 213 approval/reply fields
CREATE TABLE operational_periods (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    start_utc TEXT NOT NULL,
    end_utc TEXT NOT NULL CHECK (end_utc > start_utc),
    notes TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'void')),
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL
);
CREATE INDEX idx_op_start ON operational_periods(start_utc);

CREATE TABLE op_resources (
    id TEXT PRIMARY KEY NOT NULL,
    op_id TEXT NOT NULL REFERENCES operational_periods(id),
    sort_order INTEGER NOT NULL,
    name TEXT NOT NULL DEFAULT '',
    ics_position TEXT NOT NULL DEFAULT '',
    home_agency TEXT NOT NULL DEFAULT ''
);
CREATE INDEX idx_op_resources_op ON op_resources(op_id, sort_order);

-- No REFERENCES clause: SQLite cannot DROP a column that has one (down migration). Checked in code.
ALTER TABLE entries ADD COLUMN op_id TEXT;
CREATE INDEX idx_entries_op ON entries(op_id);

ALTER TABLE entry_emergency ADD COLUMN approved_by_name TEXT NOT NULL DEFAULT '';
ALTER TABLE entry_emergency ADD COLUMN approved_by_position TEXT NOT NULL DEFAULT '';
ALTER TABLE entry_emergency ADD COLUMN replied_by_name TEXT NOT NULL DEFAULT '';
ALTER TABLE entry_emergency ADD COLUMN replied_by_position TEXT NOT NULL DEFAULT '';
ALTER TABLE entry_emergency ADD COLUMN replied_at_utc TEXT;

UPDATE logbook SET schema_version = 2;
