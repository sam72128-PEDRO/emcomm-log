-- EMCOMM LOG logbook schema v3: channel plan per operational period (entered from an ICS 205)
CREATE TABLE channel_plan (
    id TEXT PRIMARY KEY NOT NULL,
    op_id TEXT NOT NULL REFERENCES operational_periods(id),
    sort_order INTEGER NOT NULL,
    zone_group TEXT NOT NULL DEFAULT '',
    channel_number TEXT NOT NULL DEFAULT '',
    function TEXT NOT NULL DEFAULT '',
    channel_name TEXT NOT NULL DEFAULT '',
    assignment TEXT NOT NULL DEFAULT '',
    rx_freq TEXT NOT NULL DEFAULT '',
    rx_nw TEXT NOT NULL DEFAULT '' CHECK (rx_nw IN ('', 'N', 'W')),
    rx_tone TEXT NOT NULL DEFAULT '',
    tx_freq TEXT NOT NULL DEFAULT '',
    tx_nw TEXT NOT NULL DEFAULT '' CHECK (tx_nw IN ('', 'N', 'W')),
    tx_tone TEXT NOT NULL DEFAULT '',
    mode TEXT NOT NULL DEFAULT '' CHECK (mode IN ('', 'A', 'D', 'M')),
    remarks TEXT NOT NULL DEFAULT ''
);
CREATE INDEX idx_channel_plan_op ON channel_plan(op_id, sort_order);

-- ICS 205 block 5 (special instructions) belongs to the period's plan.
ALTER TABLE operational_periods ADD COLUMN plan_instructions TEXT NOT NULL DEFAULT '';

UPDATE logbook SET schema_version = 3;
