//! Operational periods and the resources assigned to each (schema v2, V2 spec §3.1 and §3.3).
//! Periods are never deleted: they are voided like entries, because entries may point at them.

use crate::error::{AppError, AppResult};
use crate::fsutil::{self, now_utc};
use crate::logbook::Logbook;
use crate::models::{
    ChannelPlan, ChannelRow, EntryStatus, OpResource, OperationalPeriod, OperationalPeriodInput, PeriodSaveResult, PlanSource,
};
use crate::validate::{MAX_LONG, MAX_SHORT, multi_line, normalize_utc, parse_utc, single_line};
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};

pub const MAX_RESOURCES: usize = 200;
pub const MAX_CHANNELS: usize = 200;

const PERIOD_SELECT: &str = "SELECT id, name, start_utc, end_utc, notes, status, created_at_utc, updated_at_utc FROM operational_periods";

fn row_to_period(r: &Row) -> rusqlite::Result<OperationalPeriod> {
    let status: String = r.get(5)?;
    Ok(OperationalPeriod {
        id: r.get(0)?,
        name: r.get(1)?,
        start_utc: r.get(2)?,
        end_utc: r.get(3)?,
        notes: r.get(4)?,
        status: if status == "void" { EntryStatus::Void } else { EntryStatus::Active },
        created_at_utc: r.get(6)?,
        updated_at_utc: r.get(7)?,
    })
}

pub fn list_periods(conn: &Connection) -> AppResult<Vec<OperationalPeriod>> {
    let mut stmt = conn.prepare(&format!("{PERIOD_SELECT} ORDER BY start_utc, name"))?;
    let rows = stmt.query_map([], row_to_period)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

pub fn get_period(conn: &Connection, id: &str) -> AppResult<OperationalPeriod> {
    conn.query_row(&format!("{PERIOD_SELECT} WHERE id=?1"), params![id], row_to_period)
        .optional()?
        .ok_or_else(|| AppError::not_found("Operational period not found"))
}

/// The period an operator is "in" at `now`: the active period containing it (the latest-starting one
/// when several overlap for a handover). None between periods: entries are then placed by event time,
/// so nothing logged after a period ends is attached to it by accident.
pub fn current_period(conn: &Connection, now: &str) -> AppResult<Option<OperationalPeriod>> {
    Ok(conn
        .query_row(
            &format!("{PERIOD_SELECT} WHERE status='active' AND start_utc <= ?1 AND end_utc > ?1 ORDER BY start_utc DESC LIMIT 1"),
            params![now],
            row_to_period,
        )
        .optional()?)
}

pub fn list_resources(conn: &Connection, op_id: &str) -> AppResult<Vec<OpResource>> {
    let mut stmt = conn.prepare("SELECT name, ics_position, home_agency FROM op_resources WHERE op_id=?1 ORDER BY sort_order")?;
    let rows = stmt.query_map(params![op_id], |r| Ok(OpResource { name: r.get(0)?, ics_position: r.get(1)?, home_agency: r.get(2)? }))?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

fn validate_period(input: &OperationalPeriodInput) -> AppResult<OperationalPeriodInput> {
    let name = single_line("name", &input.name, 80)?;
    if name.is_empty() {
        return Err(AppError::validation("name", "Give the operational period a name (for example \"OP 1\")"));
    }
    let start = normalize_utc("start_utc", &input.start_utc)?;
    let end = normalize_utc("end_utc", &input.end_utc)?;
    if parse_utc(&end) <= parse_utc(&start) {
        return Err(AppError::validation("end_utc", "The period must end after it starts"));
    }
    Ok(OperationalPeriodInput { name, start_utc: start, end_utc: end, notes: multi_line("notes", &input.notes, MAX_LONG)? })
}

fn overlaps(conn: &Connection, id: &str, start: &str, end: &str) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT name FROM operational_periods WHERE status='active' AND id<>?1 AND start_utc < ?3 AND end_utc > ?2 ORDER BY start_utc",
    )?;
    let rows = stmt.query_map(params![id, start, end], |r| r.get::<_, String>(0))?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

const PLAN_COLS: &str =
    "zone_group, channel_number, function, channel_name, assignment, rx_freq, rx_nw, rx_tone, tx_freq, tx_nw, tx_tone, mode, remarks";

pub fn read_plan(conn: &Connection, op_id: &str) -> AppResult<ChannelPlan> {
    let special_instructions: String = conn
        .query_row("SELECT plan_instructions FROM operational_periods WHERE id=?1", params![op_id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| AppError::not_found("Operational period not found"))?;
    let mut stmt = conn.prepare(&format!("SELECT {PLAN_COLS} FROM channel_plan WHERE op_id=?1 ORDER BY sort_order"))?;
    let rows = stmt
        .query_map(params![op_id], |r| {
            Ok(ChannelRow {
                zone_group: r.get(0)?,
                channel_number: r.get(1)?,
                function: r.get(2)?,
                channel_name: r.get(3)?,
                assignment: r.get(4)?,
                rx_freq: r.get(5)?,
                rx_nw: r.get(6)?,
                rx_tone: r.get(7)?,
                tx_freq: r.get(8)?,
                tx_nw: r.get(9)?,
                tx_tone: r.get(10)?,
                mode: r.get(11)?,
                remarks: r.get(12)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(ChannelPlan { rows, special_instructions })
}

/// Periods with a non-empty channel plan, for "copy from another logbook" (any schema >= 3 database,
/// opened read-only by the caller). Older logbooks simply have none.
pub fn plan_sources(conn: &Connection) -> AppResult<Vec<PlanSource>> {
    let has: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='channel_plan'", [], |r| r.get(0))?;
    if has == 0 {
        return Ok(vec![]);
    }
    let mut stmt = conn.prepare(
        "SELECT p.id, p.name, p.start_utc, COUNT(c.id) FROM operational_periods p JOIN channel_plan c ON c.op_id = p.id
         WHERE p.status='active' GROUP BY p.id ORDER BY p.start_utc DESC",
    )?;
    let rows = stmt.query_map([], |r| Ok(PlanSource { op_id: r.get(0)?, op_name: r.get(1)?, start_utc: r.get(2)?, rows: r.get(3)? }))?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

/// "155.4750" style MHz text: digits with an optional decimal part, 0 < f < 300000.
fn valid_mhz(s: &str) -> bool {
    let ok = !s.is_empty()
        && s.chars().all(|c| c.is_ascii_digit() || c == '.')
        && s.matches('.').count() <= 1
        && !s.starts_with('.')
        && !s.ends_with('.');
    ok && s.parse::<f64>().map(|f| f > 0.0 && f < 300_000.0).unwrap_or(false)
}

fn validate_channel(i: usize, r: &ChannelRow) -> AppResult<ChannelRow> {
    let f = |name: &str, v: &str, max: usize| single_line(&format!("plan.{i}.{name}"), v, max);
    let upper = |name: &str, v: &str, allowed: &[&str]| -> AppResult<String> {
        let t = v.trim().to_uppercase();
        if allowed.contains(&t.as_str()) {
            Ok(t)
        } else {
            Err(AppError::validation(&format!("plan.{i}.{name}"), format!("Row {}: use {}", i + 1, allowed[1..].join(", "))))
        }
    };
    let freq = |name: &str, v: &str| -> AppResult<String> {
        let t = v.trim().trim_end_matches("MHz").trim_end_matches("mhz").trim().to_string();
        if t.is_empty() || valid_mhz(&t) {
            Ok(t)
        } else {
            Err(AppError::validation(&format!("plan.{i}.{name}"), format!("Row {}: frequency in MHz, e.g. 155.4750", i + 1)))
        }
    };
    Ok(ChannelRow {
        zone_group: f("zone_group", &r.zone_group, 40)?,
        channel_number: f("channel_number", &r.channel_number, 20)?,
        function: f("function", &r.function, 60)?,
        channel_name: f("channel_name", &r.channel_name, 60)?,
        assignment: f("assignment", &r.assignment, MAX_SHORT)?,
        rx_freq: freq("rx_freq", &r.rx_freq)?,
        rx_nw: upper("rx_nw", &r.rx_nw, &["", "N", "W"])?,
        rx_tone: f("rx_tone", &r.rx_tone, 20)?,
        tx_freq: freq("tx_freq", &r.tx_freq)?,
        tx_nw: upper("tx_nw", &r.tx_nw, &["", "N", "W"])?,
        tx_tone: f("tx_tone", &r.tx_tone, 20)?,
        mode: upper("mode", &r.mode, &["", "A", "D", "M"])?,
        remarks: f("remarks", &r.remarks, 500)?,
    })
}

impl Logbook {
    pub fn periods(&self) -> AppResult<Vec<OperationalPeriod>> {
        list_periods(&self.conn)
    }

    pub fn current_period(&self) -> AppResult<Option<OperationalPeriod>> {
        current_period(&self.conn, &now_utc())
    }

    /// Create (`id` None) or edit a period. Overlaps with other active periods are allowed (shift
    /// handovers) and reported back so the UI can warn.
    pub fn save_period(&mut self, id: Option<&str>, input: &OperationalPeriodInput) -> AppResult<PeriodSaveResult> {
        self.ensure_writable()?;
        let v = validate_period(input)?;
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = now_utc();
        let id = match id {
            Some(id) => {
                let existing = get_period(&tx, id)?;
                if existing.status == EntryStatus::Void {
                    return Err(AppError::validation("status", "This period is voided. Restore it before editing."));
                }
                tx.execute(
                    "UPDATE operational_periods SET name=?2, start_utc=?3, end_utc=?4, notes=?5, updated_at_utc=?6 WHERE id=?1",
                    params![id, v.name, v.start_utc, v.end_utc, v.notes, now],
                )?;
                id.to_string()
            }
            None => {
                let id = fsutil::new_uuid();
                tx.execute(
                    "INSERT INTO operational_periods(id, name, start_utc, end_utc, notes, status, created_at_utc, updated_at_utc)
                     VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?6)",
                    params![id, v.name, v.start_utc, v.end_utc, v.notes, now],
                )?;
                id
            }
        };
        tx.execute("UPDATE logbook SET updated_at_utc=?1", params![now])?;
        let period = get_period(&tx, &id)?;
        let overlaps = overlaps(&tx, &id, &period.start_utc, &period.end_utc)?;
        tx.commit()?;
        Ok(PeriodSaveResult { period, overlaps })
    }

    /// Void or restore a period. Entries keep their reference either way.
    pub fn set_period_void(&mut self, id: &str, void: bool) -> AppResult<OperationalPeriod> {
        self.ensure_writable()?;
        get_period(&self.conn, id)?;
        let now = now_utc();
        self.conn.execute(
            "UPDATE operational_periods SET status=?2, updated_at_utc=?3 WHERE id=?1",
            params![id, if void { "void" } else { "active" }, now],
        )?;
        self.conn.execute("UPDATE logbook SET updated_at_utc=?1", params![now])?;
        get_period(&self.conn, id)
    }

    pub fn period_resources(&self, op_id: &str) -> AppResult<Vec<OpResource>> {
        list_resources(&self.conn, op_id)
    }

    /// Replace the resources-assigned list of a period (ICS 214 block 6). Blank rows are dropped.
    pub fn set_period_resources(&mut self, op_id: &str, rows: &[OpResource]) -> AppResult<Vec<OpResource>> {
        self.ensure_writable()?;
        let mut clean = Vec::new();
        for (i, r) in rows.iter().enumerate() {
            let f = |name: &str, v: &str| single_line(&format!("resources.{i}.{name}"), v, MAX_SHORT);
            let row = OpResource {
                name: f("name", &r.name)?,
                ics_position: f("ics_position", &r.ics_position)?,
                home_agency: f("home_agency", &r.home_agency)?,
            };
            if !(row.name.is_empty() && row.ics_position.is_empty() && row.home_agency.is_empty()) {
                clean.push(row);
            }
        }
        if clean.len() > MAX_RESOURCES {
            return Err(AppError::validation("resources", format!("At most {MAX_RESOURCES} resources per period")));
        }
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        get_period(&tx, op_id)?;
        tx.execute("DELETE FROM op_resources WHERE op_id=?1", params![op_id])?;
        for (i, r) in clean.iter().enumerate() {
            tx.execute(
                "INSERT INTO op_resources(id, op_id, sort_order, name, ics_position, home_agency) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![fsutil::new_uuid(), op_id, i as i64, r.name, r.ics_position, r.home_agency],
            )?;
        }
        tx.execute("UPDATE logbook SET updated_at_utc=?1", params![now_utc()])?;
        tx.commit()?;
        Ok(clean)
    }

    pub fn period_plan(&self, op_id: &str) -> AppResult<ChannelPlan> {
        read_plan(&self.conn, op_id)
    }

    /// Replace a period's channel plan (rows in order + special instructions). Blank rows are dropped.
    pub fn set_period_plan(&mut self, op_id: &str, plan: &ChannelPlan) -> AppResult<ChannelPlan> {
        self.ensure_writable()?;
        let mut rows = Vec::new();
        for (i, r) in plan.rows.iter().enumerate() {
            let v = validate_channel(i, r)?;
            if v != ChannelRow::default() {
                rows.push(v);
            }
        }
        if rows.len() > MAX_CHANNELS {
            return Err(AppError::validation("plan", format!("At most {MAX_CHANNELS} channels per period")));
        }
        let instructions = multi_line("special_instructions", &plan.special_instructions, MAX_LONG)?;
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        get_period(&tx, op_id)?;
        tx.execute("DELETE FROM channel_plan WHERE op_id=?1", params![op_id])?;
        for (i, r) in rows.iter().enumerate() {
            tx.execute(
                &format!("INSERT INTO channel_plan(id, op_id, sort_order, {PLAN_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)"),
                params![
                    fsutil::new_uuid(),
                    op_id,
                    i as i64,
                    r.zone_group,
                    r.channel_number,
                    r.function,
                    r.channel_name,
                    r.assignment,
                    r.rx_freq,
                    r.rx_nw,
                    r.rx_tone,
                    r.tx_freq,
                    r.tx_nw,
                    r.tx_tone,
                    r.mode,
                    r.remarks
                ],
            )?;
        }
        let now = now_utc();
        tx.execute(
            "UPDATE operational_periods SET plan_instructions=?2, updated_at_utc=?3 WHERE id=?1",
            params![op_id, instructions, now],
        )?;
        tx.execute("UPDATE logbook SET updated_at_utc=?1", params![now])?;
        tx.commit()?;
        read_plan(&self.conn, op_id)
    }
}
