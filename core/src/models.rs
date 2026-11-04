use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

macro_rules! str_enum {
    ($name:ident { $($variant:ident => $s:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }
        impl $name {
            pub fn as_str(&self) -> &'static str { match self { $($name::$variant => $s),+ } }
            pub fn parse(s: &str) -> Option<Self> { match s { $($s => Some($name::$variant),)+ _ => None } }
            pub const ALL: &'static [$name] = &[$($name::$variant),+];
        }
    };
}

str_enum!(LogbookType { General => "general", Ham => "ham", Emergency => "emergency" });
str_enum!(EntryKind { Traffic => "traffic", Message => "message", Status => "status", Note => "note", Qso => "qso", Activity => "activity" });
str_enum!(Direction { Received => "received", Sent => "sent", Internal => "internal", Other => "other" });
str_enum!(Priority { Routine => "routine", Priority => "priority", Emergency => "emergency" });
str_enum!(Handling {
    Pending => "pending", Sent => "sent", AwaitingAck => "awaiting_ack", Acknowledged => "acknowledged",
    Relayed => "relayed", Delivered => "delivered", Closed => "closed"
});
str_enum!(EntryStatus { Active => "active", Void => "void" });
str_enum!(EmergencyRecordType { Message => "message", Activity => "activity", General => "general" });
str_enum!(FollowUpState { None => "none", Open => "open", Resolved => "resolved" });

// The enum comes from `str_enum!`, so `#[default]` cannot be attached to a variant.
#[allow(clippy::derivable_impls)]
impl Default for FollowUpState {
    fn default() -> Self {
        FollowUpState::None
    }
}

/// A follow-up is a property of the entry (2.2 FLAG-*): no task record, no notification, no timer. It is
/// independent of message handling (Pending/Sent/Acknowledged) and of Flag for Report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct FollowUp {
    pub state: FollowUpState,
    pub due_utc: Option<String>,
    pub note: String,
    /// Set by the core when the state becomes `resolved`; cleared if it is reopened.
    pub resolved_utc: Option<String>,
}

impl LogbookType {
    pub fn allowed_kinds(&self) -> &'static [EntryKind] {
        use EntryKind::*;
        match self {
            LogbookType::General => &[Traffic, Message, Status, Note],
            LogbookType::Ham => &[Qso, Traffic, Message, Status, Note],
            LogbookType::Emergency => &[Message, Activity, Traffic, Status, Note],
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            LogbookType::General => "General",
            LogbookType::Ham => "HAM",
            LogbookType::Emergency => "Emergency",
        }
    }
}

// ---------- Global (local-only) settings, profiles, presets ----------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BackupSettings {
    pub destination: Option<String>,
    pub interval_minutes: u32,
    pub on_close: bool,
    pub keep_recent: u32,
    pub keep_daily: u32,
    pub keep_weekly: u32,
}

impl Default for BackupSettings {
    fn default() -> Self {
        BackupSettings { destination: None, interval_minutes: 15, on_close: true, keep_recent: 8, keep_daily: 7, keep_weekly: 4 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GlobalSettings {
    pub schema_version: u32,
    pub wizard_completed: bool,
    pub time_display: String,
    pub clock: String,
    pub theme: String,
    pub default_logbook_parent: Option<String>,
    pub backup: BackupSettings,
    pub active_profile_id: Option<String>,
    /// Preset chosen per logbook type ("general"/"ham"/"emergency") during first run.
    pub default_preset_ids: BTreeMap<String, String>,
    /// Location & GPS preferences (2.1). No tracking setting exists: capture is always one fix on request.
    #[serde(default)]
    pub gps: GpsSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GpsSettings {
    /// "auto" (prefer a configured receiver, then OS location), "serial", "gpsd", "os", or "manual".
    pub preferred: String,
    pub serial_path: String,
    /// 0 = probe 4800 then 9600.
    pub baud: u32,
    pub serial_kind: crate::gps::SerialKind,
    pub gpsd_endpoint: String,
}

impl Default for GpsSettings {
    fn default() -> Self {
        GpsSettings {
            preferred: "auto".into(),
            serial_path: String::new(),
            baud: 0,
            serial_kind: crate::gps::SerialKind::Usb,
            gpsd_endpoint: "127.0.0.1:2947".into(),
        }
    }
}

pub const SETTINGS_SCHEMA_VERSION: u32 = 1;

impl Default for GlobalSettings {
    fn default() -> Self {
        GlobalSettings {
            schema_version: SETTINGS_SCHEMA_VERSION,
            wizard_completed: false,
            time_display: "local".into(),
            clock: "24h".into(),
            theme: "dark".into(),
            default_logbook_parent: None,
            backup: BackupSettings::default(),
            active_profile_id: None,
            default_preset_ids: BTreeMap::new(),
            gps: GpsSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct ProfileInput {
    pub name: String,
    pub callsign: String,
    pub license_class: String,
    pub organization: String,
    pub position: String,
    pub grid: String,
    #[serde(default)]
    pub license_country: String,
    #[serde(default)]
    pub unit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub callsign: String,
    pub license_class: String,
    pub organization: String,
    pub position: String,
    pub grid: String,
    #[serde(default)]
    pub license_country: String,
    #[serde(default)]
    pub unit: String,
    pub active: bool,
    pub created_at_utc: String,
    pub updated_at_utc: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct OperatorSnapshot {
    pub profile_id: Option<String>,
    pub name: String,
    pub callsign: String,
    pub license_class: String,
    pub organization: String,
    pub position: String,
    pub grid: String,
    pub license_country: String,
    pub unit: String,
}

impl From<&Profile> for OperatorSnapshot {
    fn from(p: &Profile) -> Self {
        OperatorSnapshot {
            profile_id: Some(p.id.clone()),
            name: p.name.clone(),
            callsign: p.callsign.clone(),
            license_class: p.license_class.clone(),
            organization: p.organization.clone(),
            position: p.position.clone(),
            grid: p.grid.clone(),
            license_country: p.license_country.clone(),
            unit: p.unit.clone(),
        }
    }
}

/// Optional fields a preset may hide. Core identity and time fields can never be hidden.
pub const HIDEABLE_FIELDS: &[&str] = &["channel", "location", "tags", "attachments", "handling", "flagged", "to_text", "from_text"];
/// Fields whose labels a preset may customize.
pub const LABELABLE_FIELDS: &[&str] = &["from_text", "to_text", "subject", "body", "channel", "location", "tags"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct PresetConfig {
    pub default_kind: Option<EntryKind>,
    pub default_direction: Option<Direction>,
    pub default_priority: Option<Priority>,
    pub default_channel: String,
    pub default_mode: String,
    pub hidden_fields: Vec<String>,
    pub field_labels: BTreeMap<String, String>,
    pub quick_tags: Vec<String>,
    pub channels: Vec<String>,
    pub templates: Vec<TemplateSeed>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TemplateSeed {
    pub name: String,
    pub defaults: TemplateDefaults,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct PresetInput {
    pub name: String,
    pub logbook_type: Option<LogbookType>,
    pub profile_id: Option<String>,
    pub config: PresetConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub logbook_type: LogbookType,
    pub profile_id: Option<String>,
    pub version: u32,
    pub config: PresetConfig,
    pub builtin: bool,
    pub in_use: bool,
    pub created_at_utc: String,
    pub updated_at_utc: String,
}

// ---------- Logbook ----------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct IncidentFields {
    pub incident_name: String,
    pub incident_number: String,
    pub operational_period: String,
    pub agency: String,
    pub location: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateLogbookRequest {
    pub logbook_type: LogbookType,
    pub name: String,
    pub parent_dir: String,
    pub profile_id: String,
    pub preset_id: Option<String>,
    #[serde(default)]
    pub incident: IncidentFields,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogbookInfo {
    pub id: String,
    pub logbook_type: LogbookType,
    pub name: String,
    pub incident: IncidentFields,
    pub created_at_utc: String,
    pub updated_at_utc: String,
    pub archived_at_utc: Option<String>,
    pub next_sequence: i64,
    pub schema_version: u32,
    pub origin_profile_snapshot: OperatorSnapshot,
    pub provenance: serde_json::Value,
    pub path: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct LogbookSettings {
    pub time_display_override: Option<String>,
    pub preset_id: Option<String>,
    pub preset_name: String,
    pub preset_version: u32,
    pub preset: PresetConfig,
    pub operational_notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct LogbookStats {
    pub total_active: i64,
    pub total_void: i64,
    pub today: i64,
    pub flagged: i64,
    pub favorites: i64,
    pub stations: i64,
    pub handling: BTreeMap<String, i64>,
    pub first_event_utc: Option<String>,
    pub last_event_utc: Option<String>,
    pub last_recorded_utc: Option<String>,
    pub last_sequence: i64,
    /// 2.2: active entries marked Important, open follow-ups, and those past their due time.
    #[serde(default)]
    pub important: i64,
    #[serde(default)]
    pub open_follow_ups: i64,
    #[serde(default)]
    pub overdue_follow_ups: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogbookIndexRow {
    pub id: String,
    pub path: String,
    pub name: String,
    pub logbook_type: LogbookType,
    pub incident_name: String,
    pub location: String,
    pub preset_id: Option<String>,
    pub source_uuid: Option<String>,
    pub entry_count: i64,
    pub first_event_utc: Option<String>,
    pub last_event_utc: Option<String>,
    pub last_modified_utc: Option<String>,
    pub last_opened_utc: Option<String>,
    pub archived: bool,
    pub registered_at_utc: String,
    pub available: bool,
}

// ---------- Entries ----------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct HamFields {
    pub their_call: String,
    pub station_call: String,
    pub operator_call: String,
    pub qso_start_utc: Option<String>,
    pub qso_end_utc: Option<String>,
    pub frequency_hz: Option<i64>,
    pub band: String,
    pub mode: String,
    pub submode: String,
    pub rst_sent: String,
    pub rst_rcvd: String,
    pub my_grid: String,
    pub their_grid: String,
    pub qso_notes: String,
    /// Activation references (schema v4): this station's park/summit and the other station's.
    pub my_pota_ref: String,
    pub pota_ref: String,
    pub my_sota_ref: String,
    pub sota_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct EmergencyFields {
    pub record_type: EmergencyRecordType,
    pub incident_name: String,
    pub incident_number: String,
    pub operational_period: String,
    pub message_number: String,
    pub precedence: String,
    pub originator: String,
    pub originator_position: String,
    pub recipient: String,
    pub recipient_position: String,
    pub sender: String,
    pub receiver: String,
    pub sent_at_utc: Option<String>,
    pub received_at_utc: Option<String>,
    pub reply_requested: bool,
    pub action_requested: String,
    pub response: String,
    pub resource_unit: String,
    pub assignment: String,
    pub activity_start_utc: Option<String>,
    pub activity_end_utc: Option<String>,
    pub location: String,
    pub disposition: String,
    pub preparer: String,
    // ICS 213 blocks 8 and 10 (schema v2).
    pub approved_by_name: String,
    pub approved_by_position: String,
    pub replied_by_name: String,
    pub replied_by_position: String,
    pub replied_at_utc: Option<String>,
}

impl Default for EmergencyFields {
    fn default() -> Self {
        EmergencyFields {
            record_type: EmergencyRecordType::General,
            incident_name: String::new(),
            incident_number: String::new(),
            operational_period: String::new(),
            message_number: String::new(),
            precedence: String::new(),
            originator: String::new(),
            originator_position: String::new(),
            recipient: String::new(),
            recipient_position: String::new(),
            sender: String::new(),
            receiver: String::new(),
            sent_at_utc: None,
            received_at_utc: None,
            reply_requested: false,
            action_requested: String::new(),
            response: String::new(),
            resource_unit: String::new(),
            assignment: String::new(),
            activity_start_utc: None,
            activity_end_utc: None,
            location: String::new(),
            disposition: String::new(),
            preparer: String::new(),
            approved_by_name: String::new(),
            approved_by_position: String::new(),
            replied_by_name: String::new(),
            replied_by_position: String::new(),
            replied_at_utc: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StagedAttachment {
    pub staging_id: String,
    pub display_name: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct EntryInput {
    pub kind: EntryKind,
    pub event_at_utc: String,
    pub event_timezone: String,
    pub event_utc_offset_min: i32,
    pub direction: Direction,
    pub priority: Priority,
    pub handling: Option<Handling>,
    pub flagged: bool,
    pub from_text: String,
    pub to_text: String,
    pub subject: String,
    pub body: String,
    pub channel: String,
    pub location: String,
    pub tags: Vec<String>,
    pub ham: Option<HamFields>,
    pub emergency: Option<EmergencyFields>,
    pub new_attachments: Vec<StagedAttachment>,
    /// Operational period (schema v2); None = assigned by event time when forms are made.
    pub op_id: Option<String>,
    /// Where the logging station was (schema v4); never filled automatically.
    pub operator_location: Option<Location>,
    /// A place reported in the traffic, kept apart from the station's own position.
    pub reported_location: Option<Location>,
    /// 2.2: Important (separate from Flag for Report and Favorite).
    pub important: bool,
    /// 2.2: follow-up state on the entry itself.
    pub follow_up: FollowUp,
}

impl Default for EntryInput {
    fn default() -> Self {
        EntryInput {
            kind: EntryKind::Traffic,
            event_at_utc: String::new(),
            event_timezone: "UTC".into(),
            event_utc_offset_min: 0,
            direction: Direction::Received,
            priority: Priority::Routine,
            handling: None,
            flagged: false,
            from_text: String::new(),
            to_text: String::new(),
            subject: String::new(),
            body: String::new(),
            channel: String::new(),
            location: String::new(),
            tags: vec![],
            ham: None,
            emergency: None,
            new_attachments: vec![],
            op_id: None,
            operator_location: None,
            reported_location: None,
            important: false,
            follow_up: FollowUp::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Attachment {
    pub id: String,
    pub rel_path: String,
    pub display_name: String,
    pub mime: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub added_at_utc: String,
    pub added_in_revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub id: String,
    pub sequence_no: i64,
    pub kind: EntryKind,
    pub event_at_utc: String,
    pub event_timezone: String,
    pub event_utc_offset_min: i32,
    pub recorded_at_utc: String,
    pub updated_at_utc: String,
    pub status: EntryStatus,
    pub voided_at_utc: Option<String>,
    pub void_reason: Option<String>,
    pub direction: Direction,
    pub priority: Priority,
    pub handling: Option<Handling>,
    pub flagged: bool,
    pub from_text: String,
    pub to_text: String,
    pub subject: String,
    pub body: String,
    pub channel: String,
    pub location: String,
    pub tags: Vec<String>,
    pub operator_snapshot: OperatorSnapshot,
    pub current_revision: i64,
    pub favorite: bool,
    pub ham: Option<HamFields>,
    pub emergency: Option<EmergencyFields>,
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub op_id: Option<String>,
    #[serde(default)]
    pub station_snapshot: StationSnapshot,
    #[serde(default)]
    pub operator_location: Option<Location>,
    #[serde(default)]
    pub reported_location: Option<Location>,
    #[serde(default)]
    pub important: bool,
    #[serde(default)]
    pub follow_up: FollowUp,
    /// Active links to or from this entry (2.2); not part of the entry's history.
    #[serde(default)]
    pub related_count: i64,
}

// ---------- Station, operator shifts, locations (schema v4) ----------

// Where a position came from. OS location is never labelled as GNSS: the OS may use Wi-Fi.
str_enum!(LocationSource {
    InternalGnss => "internal_gnss",
    ExternalUsbNmea => "external_usb_nmea",
    ExternalBluetoothNmea => "external_bluetooth_nmea",
    Gpsd => "gpsd",
    OsPosition => "os_position",
    Manual => "manual",
    LogbookDefault => "logbook_default",
    Reported => "reported",
    Unknown => "unknown"
});

/// An immutable location value. Coordinates are WGS84 decimal degrees, both or neither; quality
/// fields are only set when the source actually reported them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Location {
    pub latitude_deg: Option<f64>,
    pub longitude_deg: Option<f64>,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub maidenhead: String,
    pub source: LocationSource,
    /// Receiver/port or provider description (e.g. "/dev/cu.usbserial-1410 @ 4800").
    #[serde(default)]
    pub provider_id: String,
    pub captured_at_utc: Option<String>,
    pub accuracy_m: Option<f64>,
    pub altitude_m: Option<f64>,
    pub satellites: Option<u32>,
    #[serde(default)]
    pub notes: String,
}

/// The logbook's station / assignment: a desk, position or portable station, separate from the person.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct StationSnapshot {
    pub label: String,
    /// Tactical identifier such as "MARC-5 COMMS" (not a licensed callsign).
    pub tactical_id: String,
    /// The licensed station callsign used on the air, if any (club or personal). Never assumed.
    pub station_callsign: String,
    pub agency: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Station {
    pub label: String,
    pub tactical_id: String,
    pub station_callsign: String,
    pub agency: String,
    pub default_location: Option<Location>,
    /// HAM session defaults: this station's POTA park / SOTA summit while activating.
    pub pota_ref: String,
    pub sota_ref: String,
    pub updated_at_utc: String,
}

impl Station {
    pub fn snapshot(&self) -> StationSnapshot {
        StationSnapshot {
            label: self.label.clone(),
            tactical_id: self.tactical_id.clone(),
            station_callsign: self.station_callsign.clone(),
            agency: self.agency.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OperatorSession {
    pub id: String,
    pub profile_id: Option<String>,
    pub operator: OperatorSnapshot,
    pub started_at_utc: String,
    pub ended_at_utc: Option<String>,
    pub note: String,
}

// ---------- Operational periods (schema v2) ----------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OperationalPeriod {
    pub id: String,
    pub name: String,
    pub start_utc: String,
    pub end_utc: String,
    pub notes: String,
    pub status: EntryStatus,
    pub created_at_utc: String,
    pub updated_at_utc: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct OperationalPeriodInput {
    pub name: String,
    pub start_utc: String,
    pub end_utc: String,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct OpResource {
    pub name: String,
    pub ics_position: String,
    pub home_agency: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PeriodSaveResult {
    pub period: OperationalPeriod,
    /// Names of other active periods that overlap this one (allowed, but worth a warning).
    pub overlaps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SaveResult {
    pub entry: Entry,
    /// True when the request id was already committed; nothing new was written.
    pub duplicate: bool,
    /// True when Save Changes found nothing different; no revision was added.
    pub unchanged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EntryRevision {
    pub revision: i64,
    pub action: String,
    pub at_utc: String,
    pub reason: String,
    pub before: Option<serde_json::Value>,
    pub after: serde_json::Value,
    pub editor: OperatorSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct EntryQuery {
    pub text: String,
    pub from_utc: Option<String>,
    pub to_utc: Option<String>,
    pub kinds: Vec<EntryKind>,
    pub directions: Vec<Direction>,
    pub priorities: Vec<Priority>,
    pub handling: Vec<Handling>,
    pub station: String,
    pub tags: Vec<String>,
    /// "active" (default), "void", or "all"
    pub status: String,
    pub favorites_only: bool,
    pub flagged_only: bool,
    /// 2.2: only entries marked Important.
    pub important_only: bool,
    /// 2.2: "" (any), "open", "resolved" or "any" (open or resolved).
    pub follow_up: String,
    /// Restrict to specific entry ids (Export Selection).
    pub ids: Vec<String>,
    /// Operator: a profile id, or a name for entries whose profile is not on this computer.
    pub operator: String,
    /// "event" (default) or "recorded"
    pub sort: String,
    pub descending: bool,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EntryPage {
    pub total: i64,
    pub items: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct TemplateDefaults {
    pub kind: Option<EntryKind>,
    pub direction: Option<Direction>,
    pub priority: Option<Priority>,
    pub handling: Option<Handling>,
    pub flagged: bool,
    pub from_text: String,
    pub to_text: String,
    pub subject: String,
    pub body: String,
    pub channel: String,
    pub location: String,
    pub tags: Vec<String>,
    pub ham: Option<HamFields>,
    pub emergency: Option<EmergencyFields>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EntryTemplate {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub version: i64,
    pub defaults: TemplateDefaults,
    pub created_at_utc: String,
    pub updated_at_utc: String,
}

// ---------- Channel plan (schema v3), entered from an ICS 205 ----------

/// One row of an ICS 205 channel table. Frequencies are MHz as typed (decimal text).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct ChannelRow {
    pub zone_group: String,
    pub channel_number: String,
    pub function: String,
    pub channel_name: String,
    pub assignment: String,
    pub rx_freq: String,
    pub rx_nw: String,
    pub rx_tone: String,
    pub tx_freq: String,
    pub tx_nw: String,
    pub tx_tone: String,
    pub mode: String,
    pub remarks: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct ChannelPlan {
    pub rows: Vec<ChannelRow>,
    /// ICS 205 special instructions.
    pub special_instructions: String,
}

/// A period in another local logbook that has a channel plan to copy from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanSource {
    pub op_id: String,
    pub op_name: String,
    pub start_utc: String,
    pub rows: i64,
}
