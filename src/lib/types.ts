// Mirrors of the Rust models in core/src/models.rs and the command views in src-tauri/src/lib.rs.

export type LogbookType = "general" | "ham" | "emergency";
export type EntryKind = "traffic" | "message" | "status" | "note" | "qso" | "activity";
export type Direction = "received" | "sent" | "internal" | "other";
export type Priority = "routine" | "priority" | "emergency";
export type Handling =
  "pending" | "sent" | "awaiting_ack" | "acknowledged" | "relayed" | "delivered" | "closed";
export type EntryStatus = "active" | "void";
export type EmergencyRecordType = "message" | "activity" | "general";

export type ErrorCode =
  | "VALIDATION"
  | "NOT_FOUND"
  | "CONFLICT"
  | "STORAGE_UNAVAILABLE"
  | "DISK_FULL"
  | "PERMISSION_DENIED"
  | "BUSY"
  | "INTEGRITY"
  | "IO"
  | "LOCKED"
  | "UNSUPPORTED_VERSION"
  | "UNSUPPORTED_LOCATION"
  | "INVALID_PACKAGE"
  | "NO_LOGBOOK_OPEN"
  | "READ_ONLY"
  | "ALREADY_EXISTS"
  | "CANCELLED"
  | "INTERNAL";

export interface AppError {
  code: ErrorCode;
  message: string;
  field?: string;
}

export interface BackupSettings {
  destination: string | null;
  interval_minutes: number;
  on_close: boolean;
  keep_recent: number;
  keep_daily: number;
  keep_weekly: number;
}

export interface GlobalSettings {
  schema_version: number;
  wizard_completed: boolean;
  time_display: "local" | "utc";
  clock: "24h" | "12h";
  theme: "dark" | "light" | "system";
  default_logbook_parent: string | null;
  backup: BackupSettings;
  active_profile_id: string | null;
  default_preset_ids: Record<string, string>;
  gps?: GpsSettings;
}

export type SerialKind = "usb" | "bluetooth" | "internal" | "unknown";

export interface GpsSettings {
  preferred: "auto" | "serial" | "gpsd" | "os" | "manual";
  serial_path: string;
  baud: number;
  serial_kind: SerialKind;
  gpsd_endpoint: string;
}

export interface GpsSource {
  provider: "serial" | "gpsd" | "os";
  path: string;
  label: string;
  serial_kind: SerialKind | null;
  detail: string;
}

export interface GpsRequest {
  provider: "serial" | "gpsd" | "os";
  path: string;
  baud: number;
  serial_kind: SerialKind;
  gpsd_endpoint: string;
  timeout_s: number;
  max_age_s: number;
}

export type GpsStatus =
  | "fix"
  | "stale_fix"
  | "permission_denied"
  | "no_device"
  | "device_busy"
  | "no_fix"
  | "timed_out"
  | "disconnected"
  | "invalid_sentence"
  | "unavailable"
  | "cancelled";

export interface GpsOutcome {
  status: GpsStatus;
  message: string;
  location: Location | null;
  fix_age_s: number | null;
  baud: number | null;
  sentences: number;
  rejected: number;
}

export interface ProfileInput {
  name: string;
  callsign: string;
  license_class: string;
  organization: string;
  position: string;
  grid: string;
  license_country?: string;
  unit?: string;
}

export interface Profile extends ProfileInput {
  id: string;
  active: boolean;
  created_at_utc: string;
  updated_at_utc: string;
}

export interface OperatorSnapshot {
  profile_id: string | null;
  name: string;
  callsign: string;
  license_class: string;
  organization: string;
  position: string;
  grid: string;
  license_country?: string;
  unit?: string;
}

export interface HamFields {
  their_call: string;
  station_call: string;
  operator_call: string;
  qso_start_utc: string | null;
  qso_end_utc: string | null;
  frequency_hz: number | null;
  band: string;
  mode: string;
  submode: string;
  rst_sent: string;
  rst_rcvd: string;
  my_grid: string;
  their_grid: string;
  qso_notes: string;
  my_pota_ref: string;
  pota_ref: string;
  my_sota_ref: string;
  sota_ref: string;
}

export interface EmergencyFields {
  record_type: EmergencyRecordType;
  incident_name: string;
  incident_number: string;
  operational_period: string;
  message_number: string;
  precedence: string;
  originator: string;
  originator_position: string;
  recipient: string;
  recipient_position: string;
  sender: string;
  receiver: string;
  sent_at_utc: string | null;
  received_at_utc: string | null;
  reply_requested: boolean;
  action_requested: string;
  response: string;
  resource_unit: string;
  assignment: string;
  activity_start_utc: string | null;
  activity_end_utc: string | null;
  location: string;
  disposition: string;
  preparer: string;
  approved_by_name: string;
  approved_by_position: string;
  replied_by_name: string;
  replied_by_position: string;
  replied_at_utc: string | null;
}

export interface TemplateDefaults {
  kind: EntryKind | null;
  direction: Direction | null;
  priority: Priority | null;
  handling: Handling | null;
  flagged: boolean;
  from_text: string;
  to_text: string;
  subject: string;
  body: string;
  channel: string;
  location: string;
  tags: string[];
  ham: HamFields | null;
  emergency: EmergencyFields | null;
}

export interface TemplateSeed {
  name: string;
  defaults: TemplateDefaults;
}

export interface PresetConfig {
  default_kind: EntryKind | null;
  default_direction: Direction | null;
  default_priority: Priority | null;
  default_channel: string;
  default_mode: string;
  hidden_fields: string[];
  field_labels: Record<string, string>;
  quick_tags: string[];
  channels: string[];
  templates: TemplateSeed[];
}

export interface PresetInput {
  name: string;
  logbook_type: LogbookType | null;
  profile_id: string | null;
  config: PresetConfig;
}

export interface Preset {
  id: string;
  name: string;
  logbook_type: LogbookType;
  profile_id: string | null;
  version: number;
  config: PresetConfig;
  builtin: boolean;
  in_use: boolean;
  created_at_utc: string;
  updated_at_utc: string;
}

export interface IncidentFields {
  incident_name: string;
  incident_number: string;
  operational_period: string;
  agency: string;
  location: string;
}

export interface CreateLogbookRequest {
  logbook_type: LogbookType;
  name: string;
  parent_dir: string;
  profile_id: string;
  preset_id: string | null;
  incident: IncidentFields;
}

export interface LogbookInfo {
  id: string;
  logbook_type: LogbookType;
  name: string;
  incident: IncidentFields;
  created_at_utc: string;
  updated_at_utc: string;
  archived_at_utc: string | null;
  next_sequence: number;
  schema_version: number;
  origin_profile_snapshot: OperatorSnapshot;
  provenance: unknown;
  path: string;
  read_only: boolean;
}

export interface LogbookSettings {
  time_display_override: "local" | "utc" | null;
  preset_id: string | null;
  preset_name: string;
  preset_version: number;
  preset: PresetConfig;
  operational_notes: string;
}

export interface LogbookStats {
  total_active: number;
  total_void: number;
  today: number;
  flagged: number;
  favorites: number;
  stations: number;
  handling: Record<string, number>;
  first_event_utc: string | null;
  last_event_utc: string | null;
  last_recorded_utc: string | null;
  last_sequence: number;
}

export interface LogbookIndexRow {
  id: string;
  path: string;
  name: string;
  logbook_type: LogbookType;
  incident_name: string;
  location: string;
  preset_id: string | null;
  source_uuid: string | null;
  entry_count: number;
  first_event_utc: string | null;
  last_event_utc: string | null;
  last_modified_utc: string | null;
  last_opened_utc: string | null;
  archived: boolean;
  registered_at_utc: string;
  available: boolean;
}

export interface StagedAttachment {
  staging_id: string;
  display_name: string;
  size_bytes: number;
  sha256: string;
}

export interface Attachment {
  id: string;
  rel_path: string;
  display_name: string;
  mime: string;
  size_bytes: number;
  sha256: string;
  added_at_utc: string;
  added_in_revision: number;
}

export interface EntryInput {
  kind: EntryKind;
  event_at_utc: string;
  event_timezone: string;
  event_utc_offset_min: number;
  direction: Direction;
  priority: Priority;
  handling: Handling | null;
  flagged: boolean;
  from_text: string;
  to_text: string;
  subject: string;
  body: string;
  channel: string;
  location: string;
  tags: string[];
  ham: HamFields | null;
  emergency: EmergencyFields | null;
  new_attachments: StagedAttachment[];
  op_id: string | null;
  operator_location: Location | null;
  reported_location: Location | null;
}

export interface Entry {
  id: string;
  sequence_no: number;
  kind: EntryKind;
  event_at_utc: string;
  event_timezone: string;
  event_utc_offset_min: number;
  recorded_at_utc: string;
  updated_at_utc: string;
  status: EntryStatus;
  voided_at_utc: string | null;
  void_reason: string | null;
  direction: Direction;
  priority: Priority;
  handling: Handling | null;
  flagged: boolean;
  from_text: string;
  to_text: string;
  subject: string;
  body: string;
  channel: string;
  location: string;
  tags: string[];
  operator_snapshot: OperatorSnapshot;
  current_revision: number;
  favorite: boolean;
  ham: HamFields | null;
  emergency: EmergencyFields | null;
  attachments: Attachment[];
  op_id: string | null;
  station_snapshot?: StationSnapshot;
  operator_location?: Location | null;
  reported_location?: Location | null;
}

export interface SaveResult {
  entry: Entry;
  duplicate: boolean;
  unchanged: boolean;
}

export interface EntryRevision {
  revision: number;
  action: "create" | "edit" | "void" | "restore";
  at_utc: string;
  reason: string;
  before: Record<string, unknown> | null;
  after: Record<string, unknown>;
  editor: OperatorSnapshot;
}

export interface EntryQuery {
  text: string;
  from_utc: string | null;
  to_utc: string | null;
  kinds: EntryKind[];
  directions: Direction[];
  priorities: Priority[];
  handling: Handling[];
  station: string;
  tags: string[];
  status: "active" | "void" | "all";
  favorites_only: boolean;
  flagged_only: boolean;
  ids: string[];
  operator?: string;
  sort: "event" | "recorded";
  descending: boolean;
  limit: number;
  offset: number;
}

export interface EntryPage {
  total: number;
  items: Entry[];
}

export interface EntryTemplate {
  id: string;
  name: string;
  kind: string;
  version: number;
  defaults: TemplateDefaults;
  created_at_utc: string;
  updated_at_utc: string;
}

export interface StorageHealth {
  ok: boolean;
  path: string;
  journal_mode: string;
  synchronous: number;
  foreign_keys: boolean;
  error: AppError | null;
}

export interface DraftMeta {
  draft_id: string;
  logbook_id: string;
  logbook_name: string;
  editor: "new" | "edit";
  entry_id: string | null;
  entry_sequence: number | null;
  base_revision: number | null;
}

export interface Draft extends DraftMeta {
  saved_at_utc: string;
  content: Record<string, unknown>;
  intact: boolean;
}

export interface BackupRun {
  logbook_id: string;
  kind: "auto" | "close" | "manual";
  started_at_utc: string;
  finished_at_utc: string;
  ok: boolean;
  path: string | null;
  entry_count: number | null;
  message: string;
  data_marker: string | null;
}

export interface BackupStatus {
  configured: boolean;
  destination: string | null;
  interval_minutes: number;
  running: boolean;
  last: BackupRun | null;
  last_success: BackupRun | null;
  message: string | null;
}

export interface BackupOutcome {
  ok: boolean;
  skipped: boolean;
  path: string | null;
  error: AppError | null;
}

export interface ActiveView {
  info: LogbookInfo;
  settings: LogbookSettings;
  health: StorageHealth;
  notices: string[];
  drafts: Draft[];
  backup: BackupStatus;
}

export interface Bootstrap {
  settings: GlobalSettings;
  profiles: Profile[];
  presets: Preset[];
  app_version: string;
  data_dir: string;
  platform: string;
  active: ActiveView | null;
}

/** Android storage locations offered instead of folder pickers (null on desktop). */
export interface DeviceLocations {
  logbooks: string;
  tablet_backups: string;
  sd_backups: string | null;
  sd_state: string | null;
}

export interface FolderCheck {
  ok: boolean;
  kind: "local" | "network" | "cloud_sync";
  message: string | null;
}

export interface BackupListing {
  path: string;
  logbook_id: string;
  name: string;
  logbook_type: LogbookType;
  created_at_utc: string;
  kind: string;
  entry_count: number;
  total_bytes: number;
}

export interface BackupPreview {
  path: string;
  logbook_id: string;
  name: string;
  logbook_type: LogbookType;
  created_at_utc: string;
  kind: string;
  entry_count: number;
  max_sequence: number;
  attachment_count: number;
  first_event_utc: string | null;
  last_event_utc: string | null;
}

export interface PackagePreview {
  source_uuid: string;
  name: string;
  logbook_type: LogbookType;
  created_at_utc: string;
  app_version: string;
  entry_count: number;
  attachment_count: number;
}

export interface ExportPackageResult {
  path: string;
  bytes: number;
  sha256: string;
  entry_count: number;
  attachment_count: number;
}

export interface ImportOutcome {
  result: {
    path: string;
    logbook_id: string;
    source_uuid: string;
    entry_count: number;
    duplicate_of: string[];
  };
  logbook: LogbookIndexRow;
}

export type ExportFormat = "docx" | "xlsx" | "pdf" | "csv" | "txt" | "adif";

export interface ExportRequest {
  format: ExportFormat;
  scope: "all" | "range" | "filter";
  range_from: string | null;
  range_to: string | null;
  filter: EntryQuery | null;
  include_voided: boolean;
  display_zone: "local" | "utc";
  adif_preset?: AdifPreset;
  /** Shared files withhold precise coordinates (and 8-character grids) unless this is true. */
  include_precise_coordinates?: boolean;
}

export type AdifPreset = "general" | "pota_activator" | "sota_activator" | "sota_chaser";

export interface AdifSummary {
  eligible: number;
  skipped: { sequence_no: number; reason: string }[];
  station_callsigns: string[];
  operators: string[];
  refs: string[];
  first_utc: string | null;
  last_utc: string | null;
}

export interface ExportPreview {
  count: number;
  description: string;
  adif_eligible: number | null;
  adif_skipped: number | null;
  adif?: AdifSummary | null;
}

export interface ExportResult {
  path: string;
  format: ExportFormat;
  count: number;
  exported: number;
  skipped: { sequence_no: number; reason: string }[];
  warnings: string[];
  bytes: number;
}

export interface CloseResult {
  closed: boolean;
  backup: BackupOutcome | null;
}

// ---------- V2: operational periods and ICS forms ----------

export interface OperationalPeriod {
  id: string;
  name: string;
  start_utc: string;
  end_utc: string;
  notes: string;
  status: EntryStatus;
  created_at_utc: string;
  updated_at_utc: string;
}

export interface OperationalPeriodInput {
  name: string;
  start_utc: string;
  end_utc: string;
  notes: string;
}

export interface PeriodSaveResult {
  period: OperationalPeriod;
  overlaps: string[];
}

export interface OpResource {
  name: string;
  ics_position: string;
  home_agency: string;
}

export interface Form213Request {
  entry_ids: string[];
  display_zone: "local" | "utc";
}

export interface Form214Request {
  op_id: string;
  name: string;
  ics_position: string;
  home_agency: string;
  kinds: EntryKind[];
  include_details: boolean;
  prepared_by_name: string;
  prepared_by_position: string;
  display_zone: "local" | "utc";
}

export interface Form309Request {
  op_id: string;
  entry_ids: string[];
  kinds: EntryKind[];
  include_details: boolean;
  net_name: string;
  operator_name: string;
  operator_call: string;
  prepared_by_name: string;
  prepared_by_call: string;
  display_zone: "local" | "utc";
}

export interface FormPreview {
  count: number;
  skipped: { sequence_no: number; reason: string }[];
}

export interface FormResult {
  path: string;
  form: string;
  exported: number;
  pages: number;
  skipped: { sequence_no: number; reason: string }[];
  warnings: string[];
  bytes: number;
}

export interface ChannelRow {
  zone_group: string;
  channel_number: string;
  function: string;
  channel_name: string;
  assignment: string;
  rx_freq: string;
  rx_nw: string;
  rx_tone: string;
  tx_freq: string;
  tx_nw: string;
  tx_tone: string;
  mode: string;
  remarks: string;
}

export interface ChannelPlan {
  rows: ChannelRow[];
  special_instructions: string;
}

export interface PlanSource {
  op_id: string;
  op_name: string;
  start_utc: string;
  rows: number;
}

// ---------- 2.1: station, operator shifts, locations ----------

export type LocationSource =
  | "internal_gnss"
  | "external_usb_nmea"
  | "external_bluetooth_nmea"
  | "gpsd"
  | "os_position"
  | "manual"
  | "logbook_default"
  | "reported"
  | "unknown";

export interface Location {
  latitude_deg: number | null;
  longitude_deg: number | null;
  label: string;
  maidenhead: string;
  source: LocationSource;
  provider_id: string;
  captured_at_utc: string | null;
  accuracy_m: number | null;
  altitude_m: number | null;
  satellites: number | null;
  notes: string;
}

export interface StationSnapshot {
  label: string;
  tactical_id: string;
  station_callsign: string;
  agency: string;
}

export interface Station extends StationSnapshot {
  default_location: Location | null;
  pota_ref: string;
  sota_ref: string;
  updated_at_utc: string;
}

export interface OperatorSession {
  id: string;
  profile_id: string | null;
  operator: OperatorSnapshot;
  started_at_utc: string;
  ended_at_utc: string | null;
  note: string;
}

export type RecentField =
  "their_call" | "station" | "channel" | "frequency_hz" | "mode" | "pota_ref" | "sota_ref";
export type Recents = Partial<Record<RecentField, string[]>>;
