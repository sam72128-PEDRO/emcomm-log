// Operational periods (V2). Mirrors core/src/periods.rs `current_period` so the UI and the
// backend agree on which period new entries belong to.
import type * as T from "./types";

const t = (iso: string) => Date.parse(iso);

/** The active period containing `now` (latest-starting if several overlap for a handover), else
 *  null: between periods new entries are left unassigned and placed by their event time. */
export function currentPeriod(periods: T.OperationalPeriod[], now = Date.now()): T.OperationalPeriod | null {
  return (
    periods
      .filter((p) => p.status === "active" && t(p.start_utc) <= now && now < t(p.end_utc))
      .sort((a, b) => t(b.start_utc) - t(a.start_utc))[0] ?? null
  );
}

/** Whether `now` is inside the period (false once it has ended). */
export function isRunning(p: T.OperationalPeriod, now = Date.now()): boolean {
  return t(p.start_utc) <= now && now < t(p.end_utc);
}

export function periodById(periods: T.OperationalPeriod[], id: string | null | undefined) {
  return id ? (periods.find((p) => p.id === id) ?? null) : null;
}
