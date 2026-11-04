-- Roll back schema v4 to v3
ALTER TABLE entry_ham DROP COLUMN sota_ref;
ALTER TABLE entry_ham DROP COLUMN my_sota_ref;
ALTER TABLE entry_ham DROP COLUMN pota_ref;
ALTER TABLE entry_ham DROP COLUMN my_pota_ref;
ALTER TABLE entries DROP COLUMN reported_location_json;
ALTER TABLE entries DROP COLUMN operator_location_json;
ALTER TABLE entries DROP COLUMN station_snapshot_json;
DROP INDEX idx_operator_sessions_start;
DROP TABLE operator_sessions;
DROP TABLE station;
UPDATE logbook SET schema_version = 3;
