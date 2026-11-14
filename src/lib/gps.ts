// GPS settings helpers (2.1). One fix on request; never tracking.
import type * as T from "./types";
import { IS_ANDROID } from "./platform";

export const DEFAULT_GPS: T.GpsSettings = {
  preferred: "auto",
  serial_path: "",
  baud: 0,
  serial_kind: "usb",
  gpsd_endpoint: "127.0.0.1:2947",
};

// A tablet's GPS starting cold (no network assistance) can take well over 30 s outdoors.
export const GPS_TIMEOUT_S = IS_ANDROID ? 90 : 30;
export const GPS_MAX_AGE_S = 10;

/** Which source "Capture GPS" uses first: a configured receiver, else the OS location service. */
export function preferredRequest(g: T.GpsSettings | undefined, sources: T.GpsSource[]): T.GpsRequest | null {
  const s = { ...DEFAULT_GPS, ...(g ?? {}) };
  const base = {
    path: "",
    baud: s.baud,
    serial_kind: s.serial_kind,
    gpsd_endpoint: s.gpsd_endpoint,
    timeout_s: GPS_TIMEOUT_S,
    max_age_s: GPS_MAX_AGE_S,
  };
  const has = (p: T.GpsSource["provider"]) => sources.some((x) => x.provider === p);
  switch (s.preferred) {
    case "manual":
      return null;
    case "serial":
      return { ...base, provider: "serial", path: s.serial_path };
    case "gpsd":
      return { ...base, provider: "gpsd" };
    case "os":
      return { ...base, provider: "os" };
  }
  // Auto: prefer a real receiver (configured serial port, then gpsd), then the OS location service.
  if (s.serial_path) return { ...base, provider: "serial", path: s.serial_path };
  if (has("gpsd")) return { ...base, provider: "gpsd" };
  if (has("os")) return { ...base, provider: "os" };
  return null;
}

export const STATUS_TEXT: Record<T.GpsStatus, string> = {
  fix: "Fix received",
  stale_fix: "Old fix",
  permission_denied: "Permission needed",
  no_device: "Receiver not found",
  device_busy: "Receiver busy",
  no_fix: "No fix yet",
  timed_out: "Timed out",
  disconnected: "Receiver disconnected",
  invalid_sentence: "Unreadable data",
  unavailable: "Not available",
  cancelled: "Cancelled",
};
