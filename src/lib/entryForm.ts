// Editor model <-> EntryInput. The UI validates for feedback; Rust re-validates authoritatively.
import type * as T from "./types";
import { entryZone, nowIso } from "./time";
import type { Zone } from "./time";

export const KIND_LABEL: Record<T.EntryKind, string> = {
  traffic: "Traffic",
  message: "Message",
  status: "Status",
  note: "Note",
  qso: "QSO",
  activity: "Activity",
};
export const KIND_BADGE: Record<T.EntryKind, string> = {
  traffic: "TRAFFIC",
  message: "MSG",
  status: "STATUS",
  note: "NOTE",
  qso: "QSO",
  activity: "ACTIVITY",
};
export const DIRECTION_LABEL: Record<T.Direction, string> = {
  received: "Received",
  sent: "Sent",
  internal: "Internal",
  other: "Other",
};
export const DIRECTION_SHORT: Record<T.Direction, string> = {
  received: "RX",
  sent: "TX",
  internal: "INT",
  other: "—",
};
export const PRIORITY_LABEL: Record<T.Priority, string> = {
  routine: "Routine",
  priority: "Priority",
  emergency: "Emergency",
};
export const HANDLING_LABEL: Record<T.Handling, string> = {
  pending: "Pending (to send)",
  sent: "Sent",
  awaiting_ack: "Awaiting ACK",
  acknowledged: "Acknowledged",
  relayed: "Relayed",
  delivered: "Delivered",
  closed: "Closed",
};
export const TYPE_LABEL: Record<T.LogbookType, string> = {
  general: "General",
  ham: "HAM",
  emergency: "Emergency",
};

export function allowedKinds(t: T.LogbookType): T.EntryKind[] {
  if (t === "ham") return ["qso", "traffic", "message", "status", "note"];
  if (t === "emergency") return ["message", "activity", "traffic", "status", "note"];
  return ["traffic", "message", "status", "note"];
}

export const BANDS: [string, number, number][] = [
  ["2190m", 135_700, 137_800],
  ["630m", 472_000, 479_000],
  ["560m", 501_000, 504_000],
  ["160m", 1_800_000, 2_000_000],
  ["80m", 3_500_000, 4_000_000],
  ["60m", 5_060_000, 5_450_000],
  ["40m", 7_000_000, 7_300_000],
  ["30m", 10_100_000, 10_150_000],
  ["20m", 14_000_000, 14_350_000],
  ["17m", 18_068_000, 18_168_000],
  ["15m", 21_000_000, 21_450_000],
  ["12m", 24_890_000, 24_990_000],
  ["10m", 28_000_000, 29_700_000],
  ["8m", 40_000_000, 45_000_000],
  ["6m", 50_000_000, 54_000_000],
  ["5m", 54_000_001, 69_900_000],
  ["4m", 70_000_000, 71_000_000],
  ["2m", 144_000_000, 148_000_000],
  ["1.25m", 222_000_000, 225_000_000],
  ["70cm", 420_000_000, 450_000_000],
  ["33cm", 902_000_000, 928_000_000],
  ["23cm", 1_240_000_000, 1_300_000_000],
  ["13cm", 2_300_000_000, 2_450_000_000],
  ["9cm", 3_300_000_000, 3_500_000_000],
  ["6cm", 5_650_000_000, 5_925_000_000],
  ["3cm", 10_000_000_000, 10_500_000_000],
];

export const MODES = [
  "SSB",
  "USB",
  "LSB",
  "CW",
  "FM",
  "AM",
  "FT8",
  "FT4",
  "JS8",
  "RTTY",
  "PSK31",
  "OLIVIA",
  "SSTV",
  "PKT",
  "DSTAR",
  "DMR",
  "C4FM",
  "FREEDV",
  "MFSK",
  "DIGITALVOICE",
];

export function bandForHz(hz: number): string | null {
  return BANDS.find(([, lo, hi]) => hz >= lo && hz <= hi)?.[0] ?? null;
}

export type FreqUnit = "MHz" | "kHz" | "GHz";
const UNIT_EXP: Record<FreqUnit, number> = { kHz: 3, MHz: 6, GHz: 9 };

/** Exact decimal parse (no floating point) of a frequency into integer Hz. */
export function parseFrequencyHz(text: string, unit: FreqUnit): { hz: number | null; error?: string } {
  const t = text.trim();
  if (!t) return { hz: null };
  const m = /^(\d+)(?:\.(\d+))?$/.exec(t);
  if (!m) return { hz: null, error: "Enter a number such as 146.52" };
  const exp = UNIT_EXP[unit];
  const frac = (m[2] ?? "").padEnd(exp, "0");
  if (frac.length > exp && /[1-9]/.test(frac.slice(exp)))
    return { hz: null, error: "More precise than 1 Hz" };
  const hz = Number(m[1] + frac.slice(0, exp));
  if (!Number.isSafeInteger(hz) || hz <= 0) return { hz: null, error: "Frequency out of range" };
  return { hz };
}

export function formatFrequency(hz: number | null): { text: string; unit: FreqUnit } {
  if (!hz) return { text: "", unit: "MHz" };
  const whole = Math.floor(hz / 1_000_000);
  const frac = String(hz % 1_000_000)
    .padStart(6, "0")
    .replace(/0+$/, "");
  return { text: frac ? `${whole}.${frac}` : String(whole), unit: "MHz" };
}

export function blankHam(): T.HamFields {
  return {
    their_call: "",
    station_call: "",
    operator_call: "",
    qso_start_utc: null,
    qso_end_utc: null,
    frequency_hz: null,
    band: "",
    mode: "",
    submode: "",
    rst_sent: "",
    rst_rcvd: "",
    my_grid: "",
    their_grid: "",
    qso_notes: "",
  };
}

export function blankEmergency(): T.EmergencyFields {
  return {
    record_type: "general",
    incident_name: "",
    incident_number: "",
    operational_period: "",
    message_number: "",
    precedence: "",
    originator: "",
    originator_position: "",
    recipient: "",
    recipient_position: "",
    sender: "",
    receiver: "",
    sent_at_utc: null,
    received_at_utc: null,
    reply_requested: false,
    action_requested: "",
    response: "",
    resource_unit: "",
    assignment: "",
    activity_start_utc: null,
    activity_end_utc: null,
    location: "",
    disposition: "",
    preparer: "",
    approved_by_name: "",
    approved_by_position: "",
    replied_by_name: "",
    replied_by_position: "",
    replied_at_utc: null,
  };
}

export interface EntryForm {
  kind: T.EntryKind;
  event_at_utc: string;
  direction: T.Direction;
  priority: T.Priority;
  handling: T.Handling | "";
  flagged: boolean;
  from_text: string;
  to_text: string;
  subject: string;
  body: string;
  channel: string;
  location: string;
  tags: string[];
  ham: T.HamFields;
  em: T.EmergencyFields;
  freqText: string;
  freqUnit: FreqUnit;
  attachments: T.StagedAttachment[];
  /** Operational period id, or "" for none (assigned by event time when forms are made). */
  op_id: string;
}

export interface FormContext {
  type: T.LogbookType;
  preset: T.PresetConfig | null;
  profile: T.Profile | null;
  incident: T.IncidentFields;
  /** Current operational period for new entries (emergency logbooks). */
  currentOpId?: string | null;
}

export function newForm(ctx: FormContext): EntryForm {
  const p = ctx.preset;
  const kinds = allowedKinds(ctx.type);
  const kind = p?.default_kind && kinds.includes(p.default_kind) ? p.default_kind : kinds[0];
  const now = nowIso();
  const ham = blankHam();
  if (ctx.type === "ham") {
    ham.station_call = ctx.profile?.callsign ?? "";
    ham.operator_call = ctx.profile?.callsign ?? "";
    ham.my_grid = ctx.profile?.grid ?? "";
    ham.mode = p?.default_mode ?? "";
  }
  const em = blankEmergency();
  em.incident_name = ctx.incident.incident_name;
  em.incident_number = ctx.incident.incident_number;
  em.operational_period = ctx.incident.operational_period;
  em.preparer = ctx.profile ? [ctx.profile.name, ctx.profile.callsign].filter(Boolean).join(" ") : "";
  return {
    kind,
    event_at_utc: now,
    direction: p?.default_direction ?? "received",
    priority: p?.default_priority ?? "routine",
    handling: "",
    flagged: false,
    from_text: "",
    to_text: "",
    subject: "",
    body: "",
    channel: p?.default_channel ?? "",
    location: "",
    tags: [],
    ham,
    em,
    freqText: "",
    freqUnit: "MHz",
    attachments: [],
    op_id: ctx.currentOpId ?? "",
  };
}

/** Apply a template on top of a fresh form. Templates never carry timestamps. */
export function applyTemplate(form: EntryForm, d: T.TemplateDefaults, kinds: T.EntryKind[]): EntryForm {
  const f: EntryForm = structuredClone($state_snapshot_safe(form));
  if (d.kind && kinds.includes(d.kind)) f.kind = d.kind;
  if (d.direction) f.direction = d.direction;
  if (d.priority) f.priority = d.priority;
  f.handling = d.handling ?? f.handling;
  f.flagged = d.flagged || f.flagged;
  for (const k of ["from_text", "to_text", "subject", "body", "channel", "location"] as const) {
    if (d[k]) f[k] = d[k];
  }
  if (d.tags?.length) f.tags = [...new Set([...f.tags, ...d.tags])];
  if (d.ham) {
    const keep = { qso_start_utc: f.ham.qso_start_utc, qso_end_utc: f.ham.qso_end_utc };
    for (const [k, v] of Object.entries(d.ham)) {
      if (v !== "" && v !== null && v !== undefined) (f.ham as unknown as Record<string, unknown>)[k] = v;
    }
    Object.assign(f.ham, keep);
    if (d.ham.frequency_hz) Object.assign(f, freqFields(d.ham.frequency_hz));
  }
  if (d.emergency) {
    const keep = {
      sent_at_utc: f.em.sent_at_utc,
      received_at_utc: f.em.received_at_utc,
      activity_start_utc: f.em.activity_start_utc,
      activity_end_utc: f.em.activity_end_utc,
      replied_at_utc: f.em.replied_at_utc,
      incident_name: f.em.incident_name,
      incident_number: f.em.incident_number,
      operational_period: f.em.operational_period,
    };
    for (const [k, v] of Object.entries(d.emergency)) {
      if (v !== "" && v !== null && v !== undefined && v !== false)
        (f.em as unknown as Record<string, unknown>)[k] = v;
    }
    Object.assign(f.em, keep);
  }
  return f;
}

function $state_snapshot_safe<V>(v: V): V {
  return JSON.parse(JSON.stringify(v));
}

function freqFields(hz: number | null): { freqText: string; freqUnit: FreqUnit } {
  const f = formatFrequency(hz);
  return { freqText: f.text, freqUnit: f.unit };
}

export function formFromEntry(e: T.Entry): EntryForm {
  return {
    kind: e.kind,
    event_at_utc: e.event_at_utc,
    direction: e.direction,
    priority: e.priority,
    handling: e.handling ?? "",
    flagged: e.flagged,
    from_text: e.from_text,
    to_text: e.to_text,
    subject: e.subject,
    body: e.body,
    channel: e.channel,
    location: e.location,
    tags: [...e.tags],
    ham: e.ham ? { ...e.ham } : blankHam(),
    em: e.emergency ? { ...e.emergency } : blankEmergency(),
    ...freqFields(e.ham?.frequency_hz ?? null),
    attachments: [],
    op_id: e.op_id ?? "",
  };
}

/** Start a new entry from an existing one (favorites): same content, fresh times, no attachments. */
export function formCopyOf(e: T.Entry, ctx: FormContext): EntryForm {
  const f = formFromEntry(e);
  const now = nowIso();
  f.event_at_utc = now;
  f.ham.qso_start_utc = null;
  f.ham.qso_end_utc = null;
  f.em.sent_at_utc = null;
  f.em.received_at_utc = null;
  f.em.activity_start_utc = null;
  f.em.activity_end_utc = null;
  f.em.incident_name = ctx.incident.incident_name;
  f.em.incident_number = ctx.incident.incident_number;
  f.em.operational_period = ctx.incident.operational_period;
  f.em.replied_at_utc = null;
  f.op_id = ctx.currentOpId ?? "";
  return f;
}

export type FieldErrors = Record<string, string>;

export function validateForm(f: EntryForm, type: T.LogbookType): FieldErrors {
  const e: FieldErrors = {};
  if (!f.event_at_utc) e.event_at_utc = "Event time is required";
  if (!f.subject.trim()) e.subject = "Summary is required";
  if (f.subject.length > 300) e.subject = "Summary must be 300 characters or fewer";
  if (type === "ham" && f.kind === "qso") {
    const fr = parseFrequencyHz(f.freqText, f.freqUnit);
    if (fr.error) e["ham.frequency_hz"] = fr.error;
    if (!f.ham.their_call.trim()) e["ham.their_call"] = "Callsign is required for a QSO";
    const band = BANDS.find(([b]) => b === f.ham.band);
    if (fr.hz && band && (fr.hz < band[1] || fr.hz > band[2])) {
      e["ham.band"] = `${formatFrequency(fr.hz).text} MHz is outside the ${band[0]} band`;
    }
  }
  return e;
}

export function toEntryInput(f: EntryForm, type: T.LogbookType, zone: Zone): T.EntryInput {
  const z = entryZone(f.event_at_utc, zone);
  let ham: T.HamFields | null = null;
  if (type === "ham") {
    const h = { ...f.ham, frequency_hz: parseFrequencyHz(f.freqText, f.freqUnit).hz };
    if (f.kind === "qso" && !h.qso_start_utc) h.qso_start_utc = f.event_at_utc;
    // QSO fields belong only to QSO entries; profile prefills must not turn traffic into a QSO.
    ham = f.kind === "qso" ? h : null;
  }
  let emergency: T.EmergencyFields | null = null;
  if (type === "emergency") {
    emergency = { ...f.em };
    if (f.kind === "activity" && !emergency.activity_start_utc) emergency.activity_start_utc = f.event_at_utc;
  }
  return {
    kind: f.kind,
    event_at_utc: f.event_at_utc,
    ...z,
    direction: f.direction,
    priority: f.priority,
    handling: f.handling || null,
    flagged: f.flagged,
    from_text: f.from_text,
    to_text: f.to_text,
    subject: f.subject,
    body: f.body,
    channel: f.channel,
    location: f.location,
    tags: f.tags,
    ham,
    emergency,
    new_attachments: f.attachments,
    op_id: f.op_id || null,
  };
}

export function toTemplateDefaults(f: EntryForm, type: T.LogbookType): T.TemplateDefaults {
  const ham =
    type === "ham" && f.kind === "qso"
      ? {
          ...f.ham,
          frequency_hz: parseFrequencyHz(f.freqText, f.freqUnit).hz,
          qso_start_utc: null,
          qso_end_utc: null,
        }
      : null;
  const emergency =
    type === "emergency"
      ? {
          ...f.em,
          sent_at_utc: null,
          received_at_utc: null,
          activity_start_utc: null,
          activity_end_utc: null,
          replied_at_utc: null,
        }
      : null;
  return {
    kind: f.kind,
    direction: f.direction,
    priority: f.priority,
    handling: f.handling || null,
    flagged: f.flagged,
    from_text: f.from_text,
    to_text: f.to_text,
    subject: f.subject,
    body: f.body,
    channel: f.channel,
    location: f.location,
    tags: f.tags,
    ham,
    emergency,
  };
}

/** Whether the operator has typed anything worth keeping as a draft. */
export function formHasContent(f: EntryForm, baseline: EntryForm): boolean {
  const strip = (x: EntryForm) =>
    JSON.stringify({ ...x, event_at_utc: "", ham: { ...x.ham, qso_start_utc: null } });
  return strip(f) !== strip(baseline);
}
