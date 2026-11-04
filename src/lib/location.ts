// Location helpers (2.1): honest source labels, coordinate parsing and display.
import type * as T from "./types";

const SOURCE_LABEL: Record<T.LocationSource, string> = {
  internal_gnss: "Device GPS",
  external_usb_nmea: "External GPS (USB)",
  external_bluetooth_nmea: "External GPS (Bluetooth)",
  gpsd: "GPS via gpsd",
  os_position: "OS location (may be Wi-Fi based)",
  manual: "Manual",
  logbook_default: "Logbook default",
  reported: "Reported by station",
  unknown: "Unknown source",
};

export function describeSource(s: T.LocationSource): string {
  return SOURCE_LABEL[s] ?? s;
}

/** Decimal degrees, or a value with a hemisphere letter ("33.8121 N", "84.4011W"). Null when invalid. */
export function parseCoord(text: string, axis: "lat" | "lon"): number | null {
  const t = text.trim().toUpperCase().replace(/°/g, "");
  const m = /^([+-]?\d+(?:\.\d+)?)\s*([NSEW])?$/.exec(t);
  if (!m) return null;
  let v = Number(m[1]);
  const h = m[2];
  if (h) {
    if ((axis === "lat" && !"NS".includes(h)) || (axis === "lon" && !"EW".includes(h)) || v < 0) return null;
    if (h === "S" || h === "W") v = -v;
  }
  const max = axis === "lat" ? 90 : 180;
  return Number.isFinite(v) && Math.abs(v) <= max ? v : null;
}

/** Display to 6 decimals (about 0.1 m), which never implies more precision than the source gave. */
export function formatCoord(lat: number | null, lon: number | null): string {
  if (lat == null || lon == null) return "";
  return `${lat.toFixed(6)}, ${lon.toFixed(6)}`;
}

/** 6-character Maidenhead locator, computed locally (same rule as the core's `gps::maidenhead`). */
export function maidenhead(lat: number, lon: number): string {
  const x = Math.min(Math.max(lon + 180, 0), 359.999999);
  const y = Math.min(Math.max(lat + 90, 0), 179.999999);
  const A = (v: number) => String.fromCharCode(65 + Math.floor(v));
  const a = (v: number) => String.fromCharCode(97 + Math.floor(v));
  return `${A(x / 20)}${A(y / 10)}${Math.floor((x % 20) / 2)}${Math.floor(y % 10)}${a((x % 2) * 12)}${a((y % 1) * 24)}`;
}

const MEASURED: T.LocationSource[] = [
  "internal_gnss",
  "external_usb_nmea",
  "external_bluetooth_nmea",
  "gpsd",
  "os_position",
];

/** "±12 m", "accuracy unknown" for a measured fix without one, or "" for typed / reported places. */
export function accuracyNote(l: T.Location): string {
  if (l.accuracy_m != null) return `±${Math.round(l.accuracy_m)} m`;
  if (l.latitude_deg == null) return "";
  // A logbook default keeps its original source as the first word of provider_id.
  const origin =
    l.source === "logbook_default" ? (l.provider_id.split(" ")[0] as T.LocationSource) : l.source;
  return MEASURED.includes(origin) ? "accuracy unknown" : "";
}
