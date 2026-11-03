-- Roll back schema v2 to v1
ALTER TABLE entry_emergency DROP COLUMN replied_at_utc;
ALTER TABLE entry_emergency DROP COLUMN replied_by_position;
ALTER TABLE entry_emergency DROP COLUMN replied_by_name;
ALTER TABLE entry_emergency DROP COLUMN approved_by_position;
ALTER TABLE entry_emergency DROP COLUMN approved_by_name;
DROP INDEX idx_entries_op;
ALTER TABLE entries DROP COLUMN op_id;
DROP TABLE op_resources;
DROP TABLE operational_periods;
UPDATE logbook SET schema_version = 1;
