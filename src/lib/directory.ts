// 2.2 per-logbook station directory helpers (DIR-*).
import type * as T from "./types";

export const KIND_LABEL: Record<T.DirectoryKind, string> = {
  amateur_callsign: "Amateur callsign",
  tactical_identifier: "Tactical identifier",
  station: "Station / position",
  organization: "Organization",
  other: "Other",
};

export function blankDirectory(): T.DirectoryInput {
  return {
    label: "",
    kind: "other",
    identifier: "",
    agency: "",
    channel: "",
    frequency_hz: null,
    mode: "",
    aliases: [],
    tags: [],
    notes: "",
  };
}

/** Same normalisation as the core: trim, collapse spaces, upper-case. */
export const normKey = (s: string) => s.trim().split(/\s+/).join(" ").toUpperCase();

/** The text a directory item puts into a station/callsign field. */
export const fillText = (d: T.DirectoryEntry) => d.identifier || d.label;

/** The directory item a typed value refers to, if any (identifier, label or alias). */
export function matchDirectory(items: T.DirectoryEntry[], value: string): T.DirectoryEntry | null {
  const k = normKey(value);
  if (!k) return null;
  return (
    items.find((d) => normKey(d.identifier) === k) ??
    items.find((d) => normKey(d.label) === k) ??
    items.find((d) => d.aliases.some((a) => normKey(a) === k)) ??
    null
  );
}
