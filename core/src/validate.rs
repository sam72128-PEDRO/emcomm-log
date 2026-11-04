//! Authoritative input validation and normalization. The UI validates for feedback only;
//! everything persisted passes through here.

use crate::error::{AppError, AppResult};
use crate::models::*;
use chrono::{DateTime, Utc};

pub const MAX_SUBJECT: usize = 300;
pub const MAX_BODY: usize = 100_000;
pub const MAX_SHORT: usize = 120;
pub const MAX_MEDIUM: usize = 200;
pub const MAX_LONG: usize = 10_000;
pub const MAX_TAGS: usize = 20;
pub const MAX_TAG_LEN: usize = 40;
pub const MAX_ATTACHMENTS_PER_SAVE: usize = 20;

/// Parse any RFC 3339 instant and return the canonical stored form: UTC, millisecond precision, `Z`.
pub fn normalize_utc(field: &str, s: &str) -> AppResult<String> {
    let dt =
        DateTime::parse_from_rfc3339(s.trim()).map_err(|_| AppError::validation(field, format!("'{s}' is not a valid date and time")))?;
    let utc = dt.with_timezone(&Utc);
    let year = chrono::Datelike::year(&utc);
    if !(1970..=2200).contains(&year) {
        return Err(AppError::validation(field, "Date is out of the supported range (1970-2200)"));
    }
    Ok(utc.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

pub fn parse_utc(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc))
}

fn opt_time(field: &str, v: &Option<String>) -> AppResult<Option<String>> {
    match v.as_deref().map(str::trim) {
        None | Some("") => Ok(None),
        Some(s) => normalize_utc(field, s).map(Some),
    }
}

pub(crate) fn single_line(field: &str, s: &str, max: usize) -> AppResult<String> {
    let cleaned: String = s.chars().map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c }).collect();
    let cleaned: String = cleaned.chars().filter(|c| !c.is_control()).collect();
    let t = cleaned.trim().to_string();
    if t.chars().count() > max {
        return Err(AppError::validation(field, format!("Must be at most {max} characters")));
    }
    Ok(t)
}

pub(crate) fn multi_line(field: &str, s: &str, max: usize) -> AppResult<String> {
    let normalized = s.replace("\r\n", "\n").replace('\r', "\n");
    let cleaned: String = normalized.chars().filter(|c| *c == '\n' || *c == '\t' || !c.is_control()).collect();
    let t = cleaned.trim_end().trim_start_matches('\n').to_string();
    if t.chars().count() > max {
        return Err(AppError::validation(field, format!("Must be at most {max} characters")));
    }
    Ok(t)
}

pub fn normalize_callsign(field: &str, s: &str) -> AppResult<String> {
    let c = s.trim().to_ascii_uppercase();
    if c.is_empty() {
        return Ok(c);
    }
    let ok = c.len() >= 3
        && c.len() <= 20
        && c.split('/').all(|p| !p.is_empty() && p.chars().all(|ch| ch.is_ascii_alphanumeric()))
        && c.chars().any(|ch| ch.is_ascii_digit());
    if !ok {
        return Err(AppError::validation(field, format!("'{}' is not a valid callsign", s.trim())));
    }
    Ok(c)
}

pub fn normalize_grid(field: &str, s: &str) -> AppResult<String> {
    let g = s.trim();
    if g.is_empty() {
        return Ok(String::new());
    }
    let chars: Vec<char> = g.chars().collect();
    let valid_len = matches!(chars.len(), 2 | 4 | 6 | 8);
    let ok = valid_len
        && chars.iter().enumerate().all(|(i, c)| match i {
            0 | 1 => ('A'..='R').contains(&c.to_ascii_uppercase()),
            2 | 3 | 6 | 7 => c.is_ascii_digit(),
            4 | 5 => ('A'..='X').contains(&c.to_ascii_uppercase()),
            _ => false,
        });
    if !ok {
        return Err(AppError::validation(field, format!("'{g}' is not a valid Maidenhead grid locator")));
    }
    Ok(chars
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if i < 2 {
                c.to_ascii_uppercase()
            } else if !(4..=5).contains(&i) {
                *c
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect())
}

fn short_ascii(field: &str, s: &str, max: usize) -> AppResult<String> {
    let t = s.trim().to_string();
    if t.len() > max || !t.chars().all(|c| (' '..='~').contains(&c)) {
        return Err(AppError::validation(field, format!("Use up to {max} plain ASCII characters")));
    }
    Ok(t)
}

fn mode_token(field: &str, s: &str) -> AppResult<String> {
    let t = s.trim().to_ascii_uppercase();
    if t.len() > 20 || !t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(AppError::validation(field, "Mode may contain only letters, digits, and dashes (max 20)"));
    }
    Ok(t)
}

/// ADIF 3.1.4 band enumeration with lower/upper edges in Hz.
pub const BANDS: &[(&str, i64, i64)] = &[
    ("2190m", 135_700, 137_800),
    ("630m", 472_000, 479_000),
    ("560m", 501_000, 504_000),
    ("160m", 1_800_000, 2_000_000),
    ("80m", 3_500_000, 4_000_000),
    ("60m", 5_060_000, 5_450_000),
    ("40m", 7_000_000, 7_300_000),
    ("30m", 10_100_000, 10_150_000),
    ("20m", 14_000_000, 14_350_000),
    ("17m", 18_068_000, 18_168_000),
    ("15m", 21_000_000, 21_450_000),
    ("12m", 24_890_000, 24_990_000),
    ("10m", 28_000_000, 29_700_000),
    ("8m", 40_000_000, 45_000_000),
    ("6m", 50_000_000, 54_000_000),
    ("5m", 54_000_001, 69_900_000),
    ("4m", 70_000_000, 71_000_000),
    ("2m", 144_000_000, 148_000_000),
    ("1.25m", 222_000_000, 225_000_000),
    ("70cm", 420_000_000, 450_000_000),
    ("33cm", 902_000_000, 928_000_000),
    ("23cm", 1_240_000_000, 1_300_000_000),
    ("13cm", 2_300_000_000, 2_450_000_000),
    ("9cm", 3_300_000_000, 3_500_000_000),
    ("6cm", 5_650_000_000, 5_925_000_000),
    ("3cm", 10_000_000_000, 10_500_000_000),
    ("1.25cm", 24_000_000_000, 24_250_000_000),
    ("6mm", 47_000_000_000, 47_200_000_000),
    ("4mm", 75_500_000_000, 81_000_000_000),
    ("2.5mm", 119_980_000_000, 123_000_000_000),
    ("2mm", 134_000_000_000, 149_000_000_000),
    ("1mm", 241_000_000_000, 250_000_000_000),
    ("submm", 300_000_000_000, 7_500_000_000_000),
];

pub fn band_for_hz(hz: i64) -> Option<&'static str> {
    BANDS.iter().find(|(_, lo, hi)| hz >= *lo && hz <= *hi).map(|(b, _, _)| *b)
}

pub fn normalize_band(field: &str, s: &str) -> AppResult<String> {
    let b = s.trim().to_ascii_lowercase();
    if b.is_empty() {
        return Ok(b);
    }
    BANDS
        .iter()
        .find(|(n, _, _)| *n == b)
        .map(|(n, _, _)| n.to_string())
        .ok_or_else(|| AppError::validation(field, format!("'{}' is not an ADIF band", s.trim())))
}

/// Format Hz as an MHz decimal string without trailing zeros (e.g. 146520000 -> "146.52").
pub fn format_mhz(hz: i64) -> String {
    let whole = hz / 1_000_000;
    let frac = hz % 1_000_000;
    if frac == 0 {
        return format!("{whole}");
    }
    let f = format!("{frac:06}");
    format!("{whole}.{}", f.trim_end_matches('0'))
}

fn normalize_tags(tags: &[String]) -> AppResult<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for t in tags {
        let t = single_line("tags", t, MAX_TAG_LEN)?;
        let t = t.replace(',', " ").trim().to_string();
        if t.is_empty() {
            continue;
        }
        if !out.iter().any(|o| o.eq_ignore_ascii_case(&t)) {
            out.push(t);
        }
    }
    if out.len() > MAX_TAGS {
        return Err(AppError::validation("tags", format!("At most {MAX_TAGS} tags")));
    }
    Ok(out)
}

fn check_order(field: &str, start: &Option<String>, end: &Option<String>) -> AppResult<()> {
    if let (Some(s), Some(e)) = (start, end)
        && e < s
    {
        return Err(AppError::validation(field, "End time is before start time"));
    }
    Ok(())
}

pub fn validate_ham(h: &HamFields, require_qso: bool) -> AppResult<HamFields> {
    let out = HamFields {
        their_call: normalize_callsign("ham.their_call", &h.their_call)?,
        station_call: normalize_callsign("ham.station_call", &h.station_call)?,
        operator_call: normalize_callsign("ham.operator_call", &h.operator_call)?,
        qso_start_utc: opt_time("ham.qso_start_utc", &h.qso_start_utc)?,
        qso_end_utc: opt_time("ham.qso_end_utc", &h.qso_end_utc)?,
        frequency_hz: match h.frequency_hz {
            None => None,
            Some(hz) if hz > 0 && hz <= 7_500_000_000_000 => Some(hz),
            Some(_) => return Err(AppError::validation("ham.frequency_hz", "Frequency is out of range")),
        },
        band: normalize_band("ham.band", &h.band)?,
        mode: mode_token("ham.mode", &h.mode)?,
        submode: mode_token("ham.submode", &h.submode)?,
        rst_sent: short_ascii("ham.rst_sent", &h.rst_sent, 10)?,
        rst_rcvd: short_ascii("ham.rst_rcvd", &h.rst_rcvd, 10)?,
        my_grid: normalize_grid("ham.my_grid", &h.my_grid)?,
        their_grid: normalize_grid("ham.their_grid", &h.their_grid)?,
        qso_notes: multi_line("ham.qso_notes", &h.qso_notes, MAX_LONG)?,
        my_pota_ref: normalize_pota_ref("ham.my_pota_ref", &h.my_pota_ref)?,
        pota_ref: normalize_pota_ref("ham.pota_ref", &h.pota_ref)?,
        my_sota_ref: normalize_sota_ref("ham.my_sota_ref", &h.my_sota_ref)?,
        sota_ref: normalize_sota_ref("ham.sota_ref", &h.sota_ref)?,
    };
    check_order("ham.qso_end_utc", &out.qso_start_utc, &out.qso_end_utc)?;
    if let (Some(hz), false) = (out.frequency_hz, out.band.is_empty()) {
        let (_, lo, hi) = BANDS.iter().find(|(n, _, _)| *n == out.band).copied().unwrap();
        if hz < lo || hz > hi {
            return Err(AppError::validation("ham.band", format!("{} MHz is outside the {} band", format_mhz(hz), out.band)));
        }
    }
    if require_qso {
        if out.their_call.is_empty() {
            return Err(AppError::validation("ham.their_call", "A QSO needs the other station's callsign"));
        }
        if out.qso_start_utc.is_none() {
            return Err(AppError::validation("ham.qso_start_utc", "A QSO needs a start time"));
        }
    }
    Ok(out)
}

pub fn validate_emergency(e: &EmergencyFields, kind: EntryKind) -> AppResult<EmergencyFields> {
    let s = |f: &str, v: &str| single_line(&format!("emergency.{f}"), v, MAX_MEDIUM);
    let m = |f: &str, v: &str| multi_line(&format!("emergency.{f}"), v, MAX_LONG);
    let record_type = match kind {
        EntryKind::Message => EmergencyRecordType::Message,
        EntryKind::Activity => EmergencyRecordType::Activity,
        _ => EmergencyRecordType::General,
    };
    let out = EmergencyFields {
        record_type,
        incident_name: s("incident_name", &e.incident_name)?,
        incident_number: s("incident_number", &e.incident_number)?,
        operational_period: s("operational_period", &e.operational_period)?,
        message_number: s("message_number", &e.message_number)?,
        precedence: s("precedence", &e.precedence)?,
        originator: s("originator", &e.originator)?,
        originator_position: s("originator_position", &e.originator_position)?,
        recipient: s("recipient", &e.recipient)?,
        recipient_position: s("recipient_position", &e.recipient_position)?,
        sender: s("sender", &e.sender)?,
        receiver: s("receiver", &e.receiver)?,
        sent_at_utc: opt_time("emergency.sent_at_utc", &e.sent_at_utc)?,
        received_at_utc: opt_time("emergency.received_at_utc", &e.received_at_utc)?,
        reply_requested: e.reply_requested,
        action_requested: m("action_requested", &e.action_requested)?,
        response: m("response", &e.response)?,
        resource_unit: s("resource_unit", &e.resource_unit)?,
        assignment: s("assignment", &e.assignment)?,
        activity_start_utc: opt_time("emergency.activity_start_utc", &e.activity_start_utc)?,
        activity_end_utc: opt_time("emergency.activity_end_utc", &e.activity_end_utc)?,
        location: s("location", &e.location)?,
        disposition: s("disposition", &e.disposition)?,
        preparer: s("preparer", &e.preparer)?,
        approved_by_name: s("approved_by_name", &e.approved_by_name)?,
        approved_by_position: s("approved_by_position", &e.approved_by_position)?,
        replied_by_name: s("replied_by_name", &e.replied_by_name)?,
        replied_by_position: s("replied_by_position", &e.replied_by_position)?,
        replied_at_utc: opt_time("emergency.replied_at_utc", &e.replied_at_utc)?,
    };
    check_order("emergency.activity_end_utc", &out.activity_start_utc, &out.activity_end_utc)?;
    if kind == EntryKind::Activity && out.activity_start_utc.is_none() {
        return Err(AppError::validation("emergency.activity_start_utc", "An activity record needs a start time"));
    }
    Ok(out)
}

fn ham_is_empty(h: &HamFields) -> bool {
    *h == HamFields::default()
}

pub fn validate_entry_input(input: &EntryInput, book: LogbookType, now: DateTime<Utc>) -> AppResult<EntryInput> {
    if !book.allowed_kinds().contains(&input.kind) {
        return Err(AppError::validation(
            "kind",
            format!("Entry type '{}' is not available in a {} logbook", input.kind.as_str(), book.label()),
        ));
    }
    if input.event_at_utc.trim().is_empty() {
        return Err(AppError::validation("event_at_utc", "Event time is required"));
    }
    let event_at_utc = normalize_utc("event_at_utc", &input.event_at_utc)?;
    if parse_utc(&event_at_utc).unwrap() > now + chrono::Duration::hours(24) {
        return Err(AppError::validation("event_at_utc", "Event time is more than 24 hours in the future"));
    }
    let tz = single_line("event_timezone", &input.event_timezone, 64)?;
    if tz.is_empty() {
        return Err(AppError::validation("event_timezone", "Time zone is required"));
    }
    if input.event_utc_offset_min.abs() > 18 * 60 {
        return Err(AppError::validation("event_utc_offset_min", "UTC offset out of range"));
    }
    let subject = single_line("subject", &input.subject, MAX_SUBJECT)?;
    if subject.is_empty() {
        return Err(AppError::validation("subject", "Summary is required"));
    }
    let ham = match (&input.ham, book) {
        (Some(h), LogbookType::Ham) => {
            let v = validate_ham(h, input.kind == EntryKind::Qso)?;
            if ham_is_empty(&v) { None } else { Some(v) }
        }
        (None, LogbookType::Ham) if input.kind == EntryKind::Qso => {
            return Err(AppError::validation("ham.their_call", "A QSO needs the other station's callsign"));
        }
        (Some(h), _) if !ham_is_empty(h) => return Err(AppError::validation("ham", "HAM fields are only available in HAM logbooks")),
        _ => None,
    };
    let emergency = match (&input.emergency, book) {
        (Some(e), LogbookType::Emergency) => Some(validate_emergency(e, input.kind)?),
        (None, LogbookType::Emergency) => Some(validate_emergency(&EmergencyFields::default(), input.kind)?),
        (Some(_), _) => return Err(AppError::validation("emergency", "Emergency fields are only available in Emergency logbooks")),
        _ => None,
    };
    if input.new_attachments.len() > MAX_ATTACHMENTS_PER_SAVE {
        return Err(AppError::validation("attachments", format!("At most {MAX_ATTACHMENTS_PER_SAVE} attachments per save")));
    }
    let mut atts = Vec::new();
    for a in &input.new_attachments {
        if uuid::Uuid::parse_str(&a.staging_id).is_err() {
            return Err(AppError::validation("attachments", "Invalid staged attachment reference"));
        }
        let name = single_line("attachments", &a.display_name, MAX_MEDIUM)?;
        atts.push(StagedAttachment { display_name: if name.is_empty() { "attachment".into() } else { name }, ..a.clone() });
    }
    Ok(EntryInput {
        kind: input.kind,
        event_at_utc,
        event_timezone: tz,
        event_utc_offset_min: input.event_utc_offset_min,
        direction: input.direction,
        priority: input.priority,
        handling: input.handling,
        flagged: input.flagged,
        from_text: single_line("from_text", &input.from_text, MAX_SHORT)?,
        to_text: single_line("to_text", &input.to_text, MAX_SHORT)?,
        subject,
        body: multi_line("body", &input.body, MAX_BODY)?,
        channel: single_line("channel", &input.channel, MAX_SHORT)?,
        location: single_line("location", &input.location, MAX_MEDIUM)?,
        tags: normalize_tags(&input.tags)?,
        ham,
        emergency,
        new_attachments: atts,
        op_id: match &input.op_id {
            Some(id) if uuid::Uuid::parse_str(id).is_err() => {
                return Err(AppError::validation("op_id", "Invalid operational period reference"));
            }
            other => other.clone(),
        },
        operator_location: input.operator_location.as_ref().map(|l| validate_location("operator_location", l)).transpose()?,
        reported_location: input.reported_location.as_ref().map(|l| validate_location("reported_location", l)).transpose()?,
    })
}

/// Template defaults never carry timestamps; strip them and validate formats only.
pub fn validate_template_defaults(d: &TemplateDefaults, book: LogbookType) -> AppResult<TemplateDefaults> {
    if let Some(k) = d.kind
        && !book.allowed_kinds().contains(&k)
    {
        return Err(AppError::validation("kind", "Entry type not available in this logbook"));
    }
    let ham = match &d.ham {
        Some(h) if book == LogbookType::Ham => {
            let mut v = validate_ham(h, false)?;
            v.qso_start_utc = None;
            v.qso_end_utc = None;
            Some(v)
        }
        Some(h) if !ham_is_empty(h) => return Err(AppError::validation("ham", "HAM fields only in HAM logbooks")),
        _ => None,
    };
    let emergency = match &d.emergency {
        Some(e) if book == LogbookType::Emergency => {
            let mut e = e.clone();
            e.sent_at_utc = None;
            e.received_at_utc = None;
            e.activity_start_utc = None;
            e.activity_end_utc = None;
            let kind = d.kind.unwrap_or(EntryKind::Message);
            let check_kind = if kind == EntryKind::Activity { EntryKind::Note } else { kind };
            let mut v = validate_emergency(&e, check_kind)?;
            v.record_type = match kind {
                EntryKind::Message => EmergencyRecordType::Message,
                EntryKind::Activity => EmergencyRecordType::Activity,
                _ => EmergencyRecordType::General,
            };
            Some(v)
        }
        Some(_) => return Err(AppError::validation("emergency", "Emergency fields only in Emergency logbooks")),
        None => None,
    };
    Ok(TemplateDefaults {
        kind: d.kind,
        direction: d.direction,
        priority: d.priority,
        handling: d.handling,
        flagged: d.flagged,
        from_text: single_line("from_text", &d.from_text, MAX_SHORT)?,
        to_text: single_line("to_text", &d.to_text, MAX_SHORT)?,
        subject: single_line("subject", &d.subject, MAX_SUBJECT)?,
        body: multi_line("body", &d.body, MAX_BODY)?,
        channel: single_line("channel", &d.channel, MAX_SHORT)?,
        location: single_line("location", &d.location, MAX_MEDIUM)?,
        tags: normalize_tags(&d.tags)?,
        ham,
        emergency,
    })
}

pub fn validate_preset_config(c: &PresetConfig, book: LogbookType) -> AppResult<PresetConfig> {
    for f in &c.hidden_fields {
        if !HIDEABLE_FIELDS.contains(&f.as_str()) {
            return Err(AppError::validation(
                "hidden_fields",
                format!("'{f}' cannot be hidden: core identity and time fields are always shown"),
            ));
        }
    }
    for (k, v) in &c.field_labels {
        if !LABELABLE_FIELDS.contains(&k.as_str()) {
            return Err(AppError::validation("field_labels", format!("'{k}' does not support a custom label")));
        }
        if v.trim().is_empty() || v.chars().count() > 40 {
            return Err(AppError::validation("field_labels", "Labels must be 1-40 characters"));
        }
    }
    if let Some(k) = c.default_kind
        && !book.allowed_kinds().contains(&k)
    {
        return Err(AppError::validation("default_kind", "Default entry type not available for this logbook type"));
    }
    let mut templates = Vec::new();
    for t in &c.templates {
        let name = single_line("templates", &t.name, 80)?;
        if name.is_empty() {
            return Err(AppError::validation("templates", "Template name is required"));
        }
        templates.push(TemplateSeed { name, defaults: validate_template_defaults(&t.defaults, book)? });
    }
    Ok(PresetConfig {
        default_kind: c.default_kind,
        default_direction: c.default_direction,
        default_priority: c.default_priority,
        default_channel: single_line("default_channel", &c.default_channel, MAX_SHORT)?,
        default_mode: mode_token("default_mode", &c.default_mode)?,
        hidden_fields: c.hidden_fields.clone(),
        field_labels: c.field_labels.iter().map(|(k, v)| (k.clone(), v.trim().to_string())).collect(),
        quick_tags: normalize_tags(&c.quick_tags)?,
        channels: c
            .channels
            .iter()
            .map(|ch| single_line("channels", ch, MAX_SHORT))
            .collect::<AppResult<Vec<_>>>()?
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect(),
        templates,
    })
}

// ---------- 2.1: locations and activation references ----------

/// Validate a location value: coordinates both or neither, finite and in WGS84 range; quality fields
/// non-negative; times RFC 3339. Coordinates keep their full precision.
pub fn validate_location(field: &str, l: &Location) -> AppResult<Location> {
    let f = |name: &str| format!("{field}.{name}");
    match (l.latitude_deg, l.longitude_deg) {
        (Some(lat), Some(lon)) => {
            if !lat.is_finite() || !(-90.0..=90.0).contains(&lat) {
                return Err(AppError::validation(&f("latitude_deg"), "Latitude must be between -90 and 90 degrees"));
            }
            if !lon.is_finite() || !(-180.0..=180.0).contains(&lon) {
                return Err(AppError::validation(&f("longitude_deg"), "Longitude must be between -180 and 180 degrees"));
            }
        }
        (None, None) => {}
        _ => return Err(AppError::validation(&f("latitude_deg"), "Enter both latitude and longitude, or neither")),
    }
    let out = Location {
        latitude_deg: l.latitude_deg,
        longitude_deg: l.longitude_deg,
        label: single_line(&f("label"), &l.label, MAX_MEDIUM)?,
        maidenhead: normalize_grid(&f("maidenhead"), &l.maidenhead)?,
        source: l.source,
        provider_id: single_line(&f("provider_id"), &l.provider_id, MAX_MEDIUM)?,
        captured_at_utc: opt_time(&f("captured_at_utc"), &l.captured_at_utc)?,
        accuracy_m: match l.accuracy_m {
            Some(a) if !a.is_finite() || a < 0.0 => {
                return Err(AppError::validation(&f("accuracy_m"), "Accuracy must be a positive number of metres"));
            }
            a => a,
        },
        altitude_m: match l.altitude_m {
            Some(a) if !a.is_finite() || !(-1000.0..=100_000.0).contains(&a) => {
                return Err(AppError::validation(&f("altitude_m"), "Altitude is out of range"));
            }
            a => a,
        },
        satellites: match l.satellites {
            Some(n) if n > 200 => return Err(AppError::validation(&f("satellites"), "Satellite count is out of range")),
            n => n,
        },
        notes: multi_line(&f("notes"), &l.notes, MAX_LONG)?,
    };
    if out.latitude_deg.is_none() && out.label.is_empty() && out.maidenhead.is_empty() {
        return Err(AppError::validation(&f("label"), "A location needs coordinates, a grid locator or a place name"));
    }
    Ok(out)
}

/// POTA park reference (ADIF 3.1.8 POTARef): program prefix (1–4 letters/digits), "-", 4–5 digits, and an
/// optional "@" + ISO 3166-2 code. Accepts current (US-0005) and older (K-0005) prefixes; upper-cases.
pub fn normalize_pota_ref(field: &str, s: &str) -> AppResult<String> {
    let t = s.trim().to_uppercase();
    if t.is_empty() {
        return Ok(t);
    }
    let (main, loc) = match t.split_once('@') {
        Some((m, l)) => (m, Some(l)),
        None => (t.as_str(), None),
    };
    let ok_main = main.split_once('-').is_some_and(|(p, n)| {
        (1..=4).contains(&p.len())
            && p.chars().all(|c| c.is_ascii_alphanumeric())
            && (4..=5).contains(&n.len())
            && n.chars().all(|c| c.is_ascii_digit())
    });
    let ok_loc = loc.is_none_or(|l| {
        (4..=6).contains(&l.len())
            && l.split_once('-').is_some_and(|(c, r)| {
                c.len() == 2 && c.chars().all(|c| c.is_ascii_alphabetic()) && !r.is_empty() && r.chars().all(|c| c.is_ascii_alphanumeric())
            })
    });
    if ok_main && ok_loc {
        Ok(t)
    } else {
        Err(AppError::validation(field, format!("'{}' is not a POTA park reference (e.g. US-0005)", s.trim())))
    }
}

/// SOTA summit reference (ADIF SOTARef): association (prefix + optional region letters), "/", region code,
/// "-", 3 digits — e.g. W2/WE-003, G/LD-003, VK3/VC-001. Upper-cases.
pub fn normalize_sota_ref(field: &str, s: &str) -> AppResult<String> {
    let t = s.trim().to_uppercase();
    if t.is_empty() {
        return Ok(t);
    }
    let ok = t.split_once('/').is_some_and(|(assoc, rest)| {
        (1..=4).contains(&assoc.len())
            && assoc.chars().all(|c| c.is_ascii_alphanumeric())
            && rest.split_once('-').is_some_and(|(region, num)| {
                (2..=3).contains(&region.len())
                    && region.chars().all(|c| c.is_ascii_alphanumeric())
                    && num.len() == 3
                    && num.chars().all(|c| c.is_ascii_digit())
            })
    });
    if ok { Ok(t) } else { Err(AppError::validation(field, format!("'{}' is not a SOTA summit reference (e.g. W2/WE-003)", s.trim()))) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> EntryInput {
        EntryInput { subject: "Test".into(), event_at_utc: "2026-11-02T14:37:00-05:00".into(), ..Default::default() }
    }

    #[test]
    fn normalizes_time_to_utc_millis() {
        assert_eq!(normalize_utc("t", "2026-11-02T14:37:00-05:00").unwrap(), "2026-11-02T19:37:00.000Z");
        assert!(normalize_utc("t", "nope").is_err());
    }

    #[test]
    fn requires_summary_and_kind_fit() {
        let now = Utc::now();
        let mut i = base();
        i.subject = "  ".into();
        assert_eq!(validate_entry_input(&i, LogbookType::General, now).unwrap_err().field.as_deref(), Some("subject"));
        let mut i = base();
        i.kind = EntryKind::Qso;
        assert!(validate_entry_input(&i, LogbookType::General, now).is_err());
    }

    #[test]
    fn rejects_far_future() {
        let mut i = base();
        i.event_at_utc = "2199-01-01T00:00:00Z".into();
        assert!(validate_entry_input(&i, LogbookType::General, Utc::now()).is_err());
    }

    #[test]
    fn ham_rules() {
        let now = Utc::now();
        let mut i = base();
        i.kind = EntryKind::Qso;
        i.ham = Some(HamFields {
            their_call: "w1aw".into(),
            qso_start_utc: Some("2026-11-02T19:00:00Z".into()),
            frequency_hz: Some(14_074_000),
            band: "20M".into(),
            their_grid: "fn31PR".into(),
            ..Default::default()
        });
        let v = validate_entry_input(&i, LogbookType::Ham, now).unwrap();
        let h = v.ham.unwrap();
        assert_eq!(h.their_call, "W1AW");
        assert_eq!(h.band, "20m");
        assert_eq!(h.their_grid, "FN31pr");
        let mut bad = i.clone();
        bad.ham.as_mut().unwrap().band = "40m".into();
        assert_eq!(validate_entry_input(&bad, LogbookType::Ham, now).unwrap_err().field.as_deref(), Some("ham.band"));
        let mut nocall = i.clone();
        nocall.ham.as_mut().unwrap().their_call = "".into();
        assert!(validate_entry_input(&nocall, LogbookType::Ham, now).is_err());
        // HAM traffic without QSO data stays valid
        let mut traffic = base();
        traffic.kind = EntryKind::Traffic;
        assert!(validate_entry_input(&traffic, LogbookType::Ham, now).unwrap().ham.is_none());
    }

    #[test]
    fn emergency_rules() {
        let now = Utc::now();
        let mut i = base();
        i.kind = EntryKind::Activity;
        assert!(validate_entry_input(&i, LogbookType::Emergency, now).is_err());
        i.emergency = Some(EmergencyFields { activity_start_utc: Some("2026-11-02T12:00:00Z".into()), ..Default::default() });
        let v = validate_entry_input(&i, LogbookType::Emergency, now).unwrap();
        assert_eq!(v.emergency.unwrap().record_type, EmergencyRecordType::Activity);
        let mut g = base();
        g.emergency = Some(EmergencyFields::default());
        assert!(validate_entry_input(&g, LogbookType::General, now).is_err());
    }

    #[test]
    fn tags_dedupe_and_limit() {
        let mut i = base();
        i.tags = vec!["RELAY".into(), "relay".into(), " ".into(), "INFO".into()];
        assert_eq!(validate_entry_input(&i, LogbookType::General, Utc::now()).unwrap().tags, vec!["RELAY", "INFO"]);
    }

    #[test]
    fn mhz_formatting_and_bands() {
        assert_eq!(format_mhz(146_520_000), "146.52");
        assert_eq!(format_mhz(14_074_000), "14.074");
        assert_eq!(format_mhz(7_000_000), "7");
        assert_eq!(band_for_hz(146_520_000), Some("2m"));
        assert_eq!(band_for_hz(100_000_000), None);
    }

    #[test]
    fn preset_cannot_hide_core_fields() {
        let c = PresetConfig { hidden_fields: vec!["event_at_utc".into()], ..Default::default() };
        assert!(validate_preset_config(&c, LogbookType::General).is_err());
        let c = PresetConfig { hidden_fields: vec!["location".into()], ..Default::default() };
        assert!(validate_preset_config(&c, LogbookType::General).is_ok());
    }
}
