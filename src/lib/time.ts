// Time display and conversion. Instants are stored in UTC; the display zone is a separate choice.

export type Zone = "local" | "utc";
export type Clock = "24h" | "12h";

const pad = (n: number, w = 2) => String(n).padStart(w, "0");

function parts(d: Date, zone: Zone) {
  return zone === "utc"
    ? {
        y: d.getUTCFullYear(),
        mo: d.getUTCMonth() + 1,
        da: d.getUTCDate(),
        h: d.getUTCHours(),
        mi: d.getUTCMinutes(),
        s: d.getUTCSeconds(),
      }
    : {
        y: d.getFullYear(),
        mo: d.getMonth() + 1,
        da: d.getDate(),
        h: d.getHours(),
        mi: d.getMinutes(),
        s: d.getSeconds(),
      };
}

export function offsetLabel(minutes: number): string {
  const sign = minutes < 0 ? "-" : "+";
  const a = Math.abs(minutes);
  return `UTC${sign}${pad(Math.floor(a / 60))}:${pad(a % 60)}`;
}

export function localOffsetMinutes(d: Date = new Date()): number {
  return -d.getTimezoneOffset();
}

export function localZoneName(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || "Local";
  } catch {
    return "Local";
  }
}

/** Unambiguous zone indicator, e.g. "UTC" or "Local (UTC-05:00)". */
export function zoneLabel(zone: Zone, at: Date = new Date()): string {
  return zone === "utc" ? "UTC" : `Local (${offsetLabel(localOffsetMinutes(at))})`;
}

export function zoneShort(zone: Zone, at: Date = new Date()): string {
  return zone === "utc" ? "Z" : offsetLabel(localOffsetMinutes(at)).replace("UTC", "");
}

function fmtTime(h: number, mi: number, s: number | null, clock: Clock): string {
  const sec = s === null ? "" : `:${pad(s)}`;
  if (clock === "24h") return `${pad(h)}:${pad(mi)}${sec}`;
  const ap = h < 12 ? "AM" : "PM";
  const h12 = h % 12 === 0 ? 12 : h % 12;
  return `${h12}:${pad(mi)}${sec} ${ap}`;
}

export interface FormatOpts {
  date?: boolean;
  seconds?: boolean;
  zone?: boolean;
}

export function formatInstant(
  iso: string | null | undefined,
  zone: Zone,
  clock: Clock,
  opts: FormatOpts = {},
): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const p = parts(d, zone);
  const date = opts.date === false ? "" : `${p.y}-${pad(p.mo)}-${pad(p.da)} `;
  const t = fmtTime(p.h, p.mi, opts.seconds ? p.s : null, clock);
  const z = opts.zone === false ? "" : zone === "utc" ? "Z" : ` ${zoneShort("local", d)}`;
  return `${date}${t}${z}`.trim();
}

export function formatClock(d: Date, zone: Zone, clock: Clock): string {
  const p = parts(d, zone);
  return `${p.y}-${pad(p.mo)}-${pad(p.da)} ${fmtTime(p.h, p.mi, null, clock)}`;
}

/** Value for <input type="datetime-local" step="1"> in the display zone. */
export function toInputValue(iso: string | null | undefined, zone: Zone): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const p = parts(d, zone);
  return `${p.y}-${pad(p.mo)}-${pad(p.da)}T${pad(p.h)}:${pad(p.mi)}:${pad(p.s)}`;
}

/** Interpret a datetime-local value in the display zone; returns a UTC ISO string or null. */
export function fromInputValue(value: string, zone: Zone): string | null {
  if (!value) return null;
  const m = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?/.exec(value);
  if (!m) return null;
  const [y, mo, da, h, mi, s] = [m[1], m[2], m[3], m[4], m[5], m[6] ?? "0"].map(Number);
  const d = zone === "utc" ? new Date(Date.UTC(y, mo - 1, da, h, mi, s)) : new Date(y, mo - 1, da, h, mi, s);
  return Number.isNaN(d.getTime()) ? null : d.toISOString();
}

/** The zone/offset recorded with an entry so the entered time is never ambiguous. */
export function entryZone(iso: string, zone: Zone): { event_timezone: string; event_utc_offset_min: number } {
  if (zone === "utc") return { event_timezone: "UTC", event_utc_offset_min: 0 };
  return { event_timezone: localZoneName(), event_utc_offset_min: localOffsetMinutes(new Date(iso)) };
}

/** UTC bounds of "today" in the display zone, for dashboard counts. */
export function todayBoundsUtc(zone: Zone, now: Date = new Date()): [string, string] {
  if (zone === "utc") {
    const start = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate());
    return [new Date(start).toISOString(), new Date(start + 86_400_000).toISOString()];
  }
  const start = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const end = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
  return [start.toISOString(), end.toISOString()];
}

export function todayDateString(zone: Zone, now: Date = new Date()): string {
  const p = parts(now, zone);
  return `${p.y}-${pad(p.mo)}-${pad(p.da)}`;
}

export function nowIso(): string {
  return new Date().toISOString();
}

export function relativeAge(iso: string | null | undefined, now: Date = new Date()): string {
  if (!iso) return "never";
  const s = Math.round((now.getTime() - new Date(iso).getTime()) / 1000);
  if (s < 45) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86_400) return `${Math.round(s / 3600)} h ago`;
  return `${Math.round(s / 86_400)} d ago`;
}
