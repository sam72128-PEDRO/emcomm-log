-- Roll back schema v3 to v2
ALTER TABLE operational_periods DROP COLUMN plan_instructions;
DROP TABLE channel_plan;
UPDATE logbook SET schema_version = 2;
