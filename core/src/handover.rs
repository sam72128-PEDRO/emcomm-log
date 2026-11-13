//! Operator handover summary (2.3). A read-only view built from the existing shift history
//! (`operator_sessions`), entries, handling status and follow-ups. Generating one never writes to the
//! logbook, and switching operators never depends on it.
//!
//! Two kinds:
//! - **Preview** — the open shift, computed now, for the outgoing operator to brief the incoming one.
//!   It separates what was logged during this shift from outstanding work the station inherited
//!   from earlier shifts (outstanding work belongs to the station, not to one operator).
//! - **Reconstructed** — a past shift, rebuilt later from its records. Summaries are not stored, so a
//!   past one is always marked as reconstructed. It shows that shift's own activity as the entries
//!   stand now; what was outstanding at that moment is not recalculated, because it cannot be shown
//!   honestly as the original picture.

use crate::error::{AppError, AppResult};
use crate::models::{EntryKind, Handling, OperatorSession, OperatorSnapshot, Priority};
use crate::periods::current_period;
use crate::station::{list_sessions, read_station};
use rusqlite::{Connection, params_from_iter, types::Value as SqlValue};
use serde::{Deserialize, Serialize};

/// Items listed per section; totals are always exact.
pub const LIST_LIMIT: usize = 100;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SummaryBasis {
    Preview,
    Reconstructed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EntryRef {
    pub id: String,
    pub sequence_no: i64,
    pub kind: EntryKind,
    pub summary: String,
    pub event_at_utc: String,
    pub recorded_at_utc: String,
    pub priority: Option<Priority>,
    pub handling: Option<Handling>,
    pub important: bool,
    pub flagged: bool,
    pub follow_up_open: bool,
    pub follow_up_due_utc: Option<String>,
    pub follow_up_note: String,
    pub overdue: bool,
    /// Logged before the outgoing shift began (inherited by this shift).
    pub inherited: bool,
    pub logged_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RefList {
    pub total: i64,
    pub items: Vec<EntryRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KindCount {
    pub kind: EntryKind,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ShiftActivity {
    /// Active entries logged during the shift (imported records excluded; see `imported`).
    pub total: i64,
    pub by_kind: Vec<KindCount>,
    pub voided: i64,
    pub imported: i64,
    /// Important, Flag for Report, or Priority/Emergency entries logged during the shift.
    pub significant: RefList,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Outstanding {
    pub not_transmitted: RefList,
    pub transmitted_no_ack: RefList,
    pub awaiting_ack: RefList,
    pub open_follow_ups: RefList,
    pub overdue_follow_ups: i64,
    pub important: RefList,
    /// How many of all the outstanding items were inherited from earlier shifts.
    pub inherited_items: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HandoverSummary {
    pub basis: SummaryBasis,
    pub generated_at_utc: String,
    pub logbook_name: String,
    pub incident_name: String,
    pub station_label: String,
    pub tactical_id: String,
    pub station_callsign: String,
    pub outgoing: Option<OperatorSnapshot>,
    pub incoming: Option<OperatorSnapshot>,
    pub shift_id: Option<String>,
    pub shift_start_utc: Option<String>,
    pub handover_at_utc: String,
    pub period_name: Option<String>,
    pub period_start_utc: Option<String>,
    pub period_end_utc: Option<String>,
    pub activity: ShiftActivity,
    /// None for a reconstructed summary (see module docs).
    pub outstanding: Option<Outstanding>,
    /// Note recorded when the outgoing shift began.
    pub shift_start_note: String,
    /// Note handed to the incoming operator (typed at handover; for a past shift, the note recorded then).
    pub handover_note: String,
    /// Plain-text rendering for the clipboard. Times are UTC.
    pub text: String,
}

fn utc_short(s: &str) -> String {
    crate::validate::parse_utc(s).map(|t| t.format("%Y-%m-%d %H:%MZ").to_string()).unwrap_or_else(|| s.to_string())
}

const REF_SELECT: &str = "SELECT e.id, e.sequence_no, e.kind, \
    COALESCE(NULLIF(trim(e.subject), ''), NULLIF(h.their_call, ''), substr(replace(e.body, char(10), ' '), 1, 80), ''), \
    e.event_at_utc, e.recorded_at_utc, e.priority, e.handling, e.important, e.flagged, e.follow_up_state, \
    e.follow_up_due_utc, e.follow_up_note, COALESCE(json_extract(e.operator_snapshot_json, '$.name'), '') \
    FROM entries e LEFT JOIN entry_ham h ON h.entry_id = e.id";

struct Ctx<'a> {
    conn: &'a Connection,
    start: Option<String>,
    now: String,
}

impl Ctx<'_> {
    fn refs(&self, cond: &str, args: Vec<SqlValue>, order: &str) -> AppResult<RefList> {
        let total: i64 =
            self.conn.query_row(&format!("SELECT COUNT(*) FROM entries e WHERE {cond}"), params_from_iter(args.iter()), |r| r.get(0))?;
        let mut stmt = self.conn.prepare(&format!("{REF_SELECT} WHERE {cond} ORDER BY {order} LIMIT {LIST_LIMIT}"))?;
        let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
            let fu: String = r.get(10)?;
            let due: Option<String> = r.get(11)?;
            let recorded: String = r.get(5)?;
            Ok(EntryRef {
                id: r.get(0)?,
                sequence_no: r.get(1)?,
                kind: EntryKind::parse(&r.get::<_, String>(2)?).unwrap_or(EntryKind::Note),
                summary: r.get(3)?,
                event_at_utc: r.get(4)?,
                inherited: self.start.as_deref().is_some_and(|s| recorded.as_str() < s),
                recorded_at_utc: recorded,
                priority: r.get::<_, Option<String>>(6)?.as_deref().and_then(Priority::parse),
                handling: r.get::<_, Option<String>>(7)?.as_deref().and_then(Handling::parse),
                important: r.get(8)?,
                flagged: r.get(9)?,
                follow_up_open: fu == "open",
                overdue: fu == "open" && due.as_deref().is_some_and(|d| d < self.now.as_str()),
                follow_up_due_utc: due,
                follow_up_note: r.get(12)?,
                logged_by: r.get(13)?,
            })
        })?;
        Ok(RefList { total, items: rows.collect::<rusqlite::Result<Vec<_>>>()? })
    }
}

/// Window condition on `recorded_at_utc` for a shift: [start, end).
fn window(start: Option<&str>, end: &str) -> (String, Vec<SqlValue>) {
    match start {
        Some(s) => ("e.recorded_at_utc >= ? AND e.recorded_at_utc < ?".into(), vec![s.to_string().into(), end.to_string().into()]),
        None => ("e.recorded_at_utc < ?".into(), vec![end.to_string().into()]),
    }
}

fn not_imported() -> &'static str {
    "NOT EXISTS (SELECT 1 FROM entry_import_source s WHERE s.entry_id = e.id)"
}

fn activity(ctx: &Ctx, start: Option<&str>, end: &str) -> AppResult<ShiftActivity> {
    let (w, args) = window(start, end);
    let mut by_kind = Vec::new();
    {
        let mut stmt = ctx.conn.prepare(&format!(
            "SELECT e.kind, COUNT(*) FROM entries e WHERE {w} AND e.status='active' AND {} GROUP BY e.kind ORDER BY COUNT(*) DESC, e.kind",
            not_imported()
        ))?;
        for r in stmt.query_map(params_from_iter(args.iter()), |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
            let (k, n) = r?;
            if let Some(kind) = EntryKind::parse(&k) {
                by_kind.push(KindCount { kind, count: n });
            }
        }
    }
    let count = |extra: &str| -> AppResult<i64> {
        Ok(ctx
            .conn
            .query_row(&format!("SELECT COUNT(*) FROM entries e WHERE {w} AND {extra}"), params_from_iter(args.iter()), |r| r.get(0))?)
    };
    let voided = count(&format!("e.status='void' AND {}", not_imported()))?;
    let imported = count("e.status='active' AND EXISTS (SELECT 1 FROM entry_import_source s WHERE s.entry_id = e.id)")?;
    let significant = ctx.refs(
        &format!(
            "{w} AND e.status='active' AND {} AND (e.important=1 OR e.flagged=1 OR e.priority IN ('priority','emergency'))",
            not_imported()
        ),
        args.clone(),
        "e.sequence_no",
    )?;
    Ok(ShiftActivity { total: by_kind.iter().map(|k| k.count).sum(), by_kind, voided, imported, significant })
}

fn outstanding(ctx: &Ctx) -> AppResult<Outstanding> {
    let by_handling = |h: &str| ctx.refs("e.status='active' AND e.handling = ?", vec![h.to_string().into()], "e.sequence_no");
    let open_follow_ups = ctx.refs(
        "e.status='active' AND e.follow_up_state='open'",
        vec![],
        "CASE WHEN e.follow_up_due_utc IS NULL THEN 1 ELSE 0 END, e.follow_up_due_utc, e.sequence_no",
    )?;
    let overdue_follow_ups: i64 = ctx.conn.query_row(
        "SELECT COUNT(*) FROM entries WHERE status='active' AND follow_up_state='open' AND follow_up_due_utc IS NOT NULL AND follow_up_due_utc < ?1",
        [&ctx.now],
        |r| r.get(0),
    )?;
    let mut o = Outstanding {
        not_transmitted: by_handling("pending")?,
        transmitted_no_ack: by_handling("sent")?,
        awaiting_ack: by_handling("awaiting_ack")?,
        open_follow_ups,
        overdue_follow_ups,
        important: ctx.refs("e.status='active' AND e.important=1", vec![], "e.sequence_no DESC")?,
        inherited_items: 0,
    };
    if let Some(s) = &ctx.start {
        o.inherited_items = ctx.conn.query_row(
            "SELECT COUNT(*) FROM entries WHERE status='active' AND recorded_at_utc < ?1
               AND (handling IN ('pending','sent','awaiting_ack') OR follow_up_state='open' OR important=1)",
            [s],
            |r| r.get(0),
        )?;
    }
    Ok(o)
}

struct Head {
    logbook_name: String,
    incident_name: String,
}

fn head(conn: &Connection) -> AppResult<Head> {
    Ok(conn.query_row("SELECT name, incident_name FROM logbook", [], |r| Ok(Head { logbook_name: r.get(0)?, incident_name: r.get(1)? }))?)
}

fn op_text(o: &Option<OperatorSnapshot>) -> String {
    match o {
        Some(o) if !o.callsign.is_empty() => format!("{} ({})", o.name, o.callsign),
        Some(o) => o.name.clone(),
        None => "—".into(),
    }
}

#[allow(clippy::too_many_arguments)]
fn build(
    conn: &Connection,
    basis: SummaryBasis,
    session: Option<&OperatorSession>,
    end: &str,
    incoming: Option<OperatorSnapshot>,
    handover_note: &str,
    with_outstanding: bool,
    now: &str,
) -> AppResult<HandoverSummary> {
    let h = head(conn)?;
    let st = read_station(conn)?;
    let start = session.map(|s| s.started_at_utc.clone());
    let ctx = Ctx { conn, start: start.clone(), now: now.to_string() };
    let period = current_period(conn, end)?;
    let activity = activity(&ctx, start.as_deref(), end)?;
    let outstanding = if with_outstanding { Some(outstanding(&ctx)?) } else { None };
    let mut s = HandoverSummary {
        basis,
        generated_at_utc: now.to_string(),
        logbook_name: h.logbook_name,
        incident_name: h.incident_name,
        station_label: st.label,
        tactical_id: st.tactical_id,
        station_callsign: st.station_callsign,
        outgoing: session.map(|s| s.operator.clone()),
        incoming,
        shift_id: session.map(|s| s.id.clone()),
        shift_start_utc: start,
        handover_at_utc: end.to_string(),
        period_name: period.as_ref().map(|p| p.name.clone()),
        period_start_utc: period.as_ref().map(|p| p.start_utc.clone()),
        period_end_utc: period.as_ref().map(|p| p.end_utc.clone()),
        activity,
        outstanding,
        shift_start_note: session.map(|s| s.note.clone()).unwrap_or_default(),
        handover_note: handover_note.trim().to_string(),
        text: String::new(),
    };
    s.text = render_text(&s);
    Ok(s)
}

/// Preview of the open shift's handover at `at_utc` (default now). Read-only.
pub fn preview(
    conn: &Connection,
    incoming: Option<OperatorSnapshot>,
    at_utc: Option<&str>,
    handover_note: &str,
) -> AppResult<HandoverSummary> {
    let now = crate::fsutil::now_utc();
    let at = match at_utc.map(str::trim).filter(|s| !s.is_empty()) {
        Some(t) => crate::validate::normalize_utc("at_utc", t)?,
        None => now.clone(),
    };
    let active = crate::station::active_session(conn)?;
    // Entries are recorded up to the handover instant; one logged in the same millisecond still counts.
    let end = bump_ms(&at);
    let mut s = build(conn, SummaryBasis::Preview, active.as_ref(), &end, incoming, handover_note, true, &now)?;
    s.handover_at_utc = at;
    s.text = render_text(&s);
    Ok(s)
}

/// A past (ended) shift, reconstructed from its records and marked as such.
pub fn reconstruct(conn: &Connection, session_id: &str) -> AppResult<HandoverSummary> {
    let sessions = list_sessions(conn)?;
    let idx = sessions.iter().position(|s| s.id == session_id).ok_or_else(|| AppError::not_found("Shift not found"))?;
    let sess = &sessions[idx];
    let Some(end) = sess.ended_at_utc.clone() else {
        return Err(AppError::validation("session_id", "This shift is still open. Use the handover preview instead."));
    };
    let next = sessions.iter().skip(idx + 1).find(|s| s.started_at_utc == end).or_else(|| sessions.get(idx + 1));
    let now = crate::fsutil::now_utc();
    build(
        conn,
        SummaryBasis::Reconstructed,
        Some(sess),
        &end,
        next.map(|n| n.operator.clone()),
        next.map(|n| n.note.as_str()).unwrap_or(""),
        false,
        &now,
    )
}

fn bump_ms(t: &str) -> String {
    crate::validate::parse_utc(t)
        .map(|d| (d + chrono::Duration::milliseconds(1)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_else(|| t.to_string())
}

fn kind_label(k: EntryKind) -> &'static str {
    match k {
        EntryKind::Traffic => "Traffic",
        EntryKind::Message => "Message",
        EntryKind::Status => "Status",
        EntryKind::Note => "Note",
        EntryKind::Qso => "QSO",
        EntryKind::Activity => "Activity",
    }
}

fn ref_line(r: &EntryRef) -> String {
    let mut tags = Vec::new();
    if let Some(h) = r.handling {
        tags.push(h.label().to_string());
    }
    if r.important {
        tags.push("Important".into());
    }
    if r.flagged {
        tags.push("Flagged".into());
    }
    if let Some(p) = r.priority.filter(|p| *p != Priority::Routine) {
        tags.push(if p == Priority::Emergency { "EMERGENCY".into() } else { "Priority".into() });
    }
    if r.follow_up_open {
        let due = r.follow_up_due_utc.as_deref().map(|d| format!(" due {}", utc_short(d))).unwrap_or_default();
        tags.push(format!("{}{due}", if r.overdue { "Follow-up OVERDUE" } else { "Follow-up" }));
    }
    if r.inherited {
        tags.push("from earlier shift".into());
    }
    let mut line = format!("  #{} {} {} — {}", r.sequence_no, utc_short(&r.event_at_utc), kind_label(r.kind), r.summary);
    if !tags.is_empty() {
        line.push_str(&format!(" [{}]", tags.join(", ")));
    }
    if r.follow_up_open && !r.follow_up_note.is_empty() {
        line.push_str(&format!("\n      Follow-up: {}", r.follow_up_note));
    }
    line
}

fn list_block(out: &mut String, title: &str, l: &RefList) {
    out.push_str(&format!("{title}: {}\n", l.total));
    for r in &l.items {
        out.push_str(&ref_line(r));
        out.push('\n');
    }
    if l.total as usize > l.items.len() {
        out.push_str(&format!("  … and {} more (see the Traffic Log)\n", l.total as usize - l.items.len()));
    }
}

pub fn render_text(s: &HandoverSummary) -> String {
    let mut o = String::new();
    match s.basis {
        SummaryBasis::Preview => o.push_str("OPERATOR HANDOVER SUMMARY\n"),
        SummaryBasis::Reconstructed => {
            o.push_str("OPERATOR HANDOVER SUMMARY — RECONSTRUCTED\n");
            o.push_str(&format!(
                "Reconstructed from records on {}. Not an original snapshot: entries are shown as they stand now, and \
                 outstanding work at the time of this handover is not recalculated.\n",
                utc_short(&s.generated_at_utc)
            ));
        }
    }
    o.push_str(&format!("Logbook: {}\n", s.logbook_name));
    if !s.incident_name.is_empty() {
        o.push_str(&format!("Incident: {}\n", s.incident_name));
    }
    let station: Vec<&str> =
        [s.station_label.as_str(), s.tactical_id.as_str(), s.station_callsign.as_str()].into_iter().filter(|x| !x.is_empty()).collect();
    if !station.is_empty() {
        o.push_str(&format!("Station: {}\n", station.join(" · ")));
    }
    o.push_str(&format!("Outgoing operator: {}\n", op_text(&s.outgoing)));
    o.push_str(&format!("Incoming operator: {}\n", op_text(&s.incoming)));
    o.push_str(&format!(
        "Shift: {} to {} (UTC)\n",
        s.shift_start_utc.as_deref().map(utc_short).unwrap_or_else(|| "no shift recorded".into()),
        utc_short(&s.handover_at_utc)
    ));
    if let Some(p) = &s.period_name {
        o.push_str(&format!(
            "Operational period: {p} ({} to {})\n",
            s.period_start_utc.as_deref().map(utc_short).unwrap_or_default(),
            s.period_end_utc.as_deref().map(utc_short).unwrap_or_default()
        ));
    }
    o.push_str("\nTHIS SHIFT\n");
    let a = &s.activity;
    if a.total == 0 {
        o.push_str("No entries were logged during this shift.\n");
    } else {
        let kinds: Vec<String> = a.by_kind.iter().map(|k| format!("{} {}", k.count, kind_label(k.kind))).collect();
        o.push_str(&format!("Entries logged: {} ({})\n", a.total, kinds.join(", ")));
    }
    if a.voided > 0 {
        o.push_str(&format!("Voided: {}\n", a.voided));
    }
    if a.imported > 0 {
        o.push_str(&format!("Imported records (ADIF): {}\n", a.imported));
    }
    if a.significant.total > 0 {
        list_block(&mut o, "Significant (Important, Flagged, Priority/Emergency)", &a.significant);
    }
    if let Some(x) = &s.outstanding {
        o.push_str("\nOUTSTANDING AT THE STATION (all shifts)\n");
        if x.inherited_items > 0 {
            let n = x.inherited_items;
            o.push_str(&format!(
                "{n} {} below {} logged before this shift (inherited).\n",
                if n == 1 { "entry" } else { "entries" },
                if n == 1 { "was" } else { "were" }
            ));
        }
        list_block(&mut o, "Not yet transmitted (Pending)", &x.not_transmitted);
        list_block(&mut o, "Transmitted, no acknowledgment recorded (Sent)", &x.transmitted_no_ack);
        list_block(&mut o, "Awaiting acknowledgment", &x.awaiting_ack);
        list_block(&mut o, &format!("Open follow-ups ({} overdue)", x.overdue_follow_ups), &x.open_follow_ups);
        list_block(&mut o, "Important entries", &x.important);
    }
    if !s.shift_start_note.is_empty() || !s.handover_note.is_empty() {
        o.push_str("\nNOTES\n");
        if !s.shift_start_note.is_empty() {
            o.push_str(&format!("At start of this shift: {}\n", s.shift_start_note));
        }
        if !s.handover_note.is_empty() {
            o.push_str(&format!("Handover note: {}\n", s.handover_note));
        }
    }
    o.push_str(&format!("\nGenerated {} by EMCOMM LOG {}\n", utc_short(&s.generated_at_utc), crate::logbook::APP_VERSION));
    o
}
