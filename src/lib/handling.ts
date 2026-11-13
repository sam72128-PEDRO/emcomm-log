import { app } from "./state.svelte";
import { api, newId, toAppError } from "./api";
import type * as T from "./types";
import { confirm } from "./confirm.svelte";
import { formFromEntry, HANDLING_LABEL, toEntryInput } from "./entryForm";

/** Position in the transmission / acknowledgment lifecycle (mirrors the core's `Handling::rank`). */
export const HANDLING_RANK: Record<T.Handling, number> = {
  pending: 0,
  sent: 1,
  awaiting_ack: 2,
  relayed: 2,
  acknowledged: 3,
  delivered: 3,
  closed: 4,
};

/** Counted as Pending Traffic (not transmitted, or transmitted without a recorded acknowledgment). */
export const OUTSTANDING_HANDLING: T.Handling[] = ["pending", "sent", "awaiting_ack"];

/**
 * True when the change would undo recorded transmission or acknowledgment progress. The core then
 * requires a reason, so the earlier status is never silently replaced (it stays in the history).
 */
export function isHandlingRegression(before: T.Handling | null, after: T.Handling | null): boolean {
  if (!before || HANDLING_RANK[before] === 0) return false;
  if (!after) return true;
  return HANDLING_RANK[after] < HANDLING_RANK[before];
}

/** Plain-language outstanding action (mirrors the core's `Handling::outstanding_action`). */
export function outstandingAction(h: T.Handling | null): string {
  switch (h) {
    case "pending":
      return "Transmit";
    case "sent":
      return "None recorded (mark Awaiting ACK if one is expected)";
    case "awaiting_ack":
      return "Record ACK when received";
    default:
      return "";
  }
}

/** Change only the handling status of an entry, as a normal Save Changes with its own revision. */
export async function changeHandling(e: T.Entry, h: T.Handling | null): Promise<boolean> {
  if (!app.active) return false;
  const label = h ? HANDLING_LABEL[h] : "not tracked";
  let reason = `Handling changed to ${label}`;
  if (isHandlingRegression(e.handling, h)) {
    const from = e.handling ? HANDLING_LABEL[e.handling] : "not tracked";
    const a = await confirm.ask({
      title: "Correct the message status?",
      message: `Entry #${e.sequence_no} is recorded as ${from}. Moving it back to ${label} is a correction.`,
      detail: "The earlier status stays in the revision history. Give the reason for the correction.",
      input: { label: "Reason (required)", value: "", maxLength: 400 },
      buttons: [
        { id: "ok", label: "Record correction", tone: "danger" },
        { id: "cancel", label: "Keep status", tone: "primary" },
      ],
      cancelId: "cancel",
    });
    if (a.id !== "ok") return false;
    if (!a.value.trim()) {
      app.toast("error", "Status not changed: a reason is required to move a message status back.");
      return false;
    }
    reason = `Status corrected from ${from} to ${label}: ${a.value.trim()}`;
  }
  const f = formFromEntry(e);
  f.handling = h ?? "";
  const input = toEntryInput(f, app.active.info.logbook_type, app.zone);
  input.event_timezone = e.event_timezone;
  input.event_utc_offset_min = e.event_utc_offset_min;
  try {
    const r = await api.saveChanges(e.id, e.current_revision, input, reason, newId(), null);
    app.saveStatus = {
      kind: "saved",
      seq: r.entry.sequence_no,
      at: r.entry.updated_at_utc,
      edit: true,
      duplicate: false,
    };
    app.announce(`Entry ${r.entry.sequence_no} marked ${label}`);
    app.bumpData();
    return true;
  } catch (err) {
    const ae = toAppError(err);
    app.noteSaveFailure(ae);
    app.toast("error", `Status not changed: ${ae.message}`);
    return false;
  }
}

export function nextHandling(h: T.Handling | null): T.Handling {
  return h === "pending" ? "sent" : h === "sent" ? "awaiting_ack" : "acknowledged";
}

/**
 * 2.2: change only Important and/or the follow-up of an entry, as a normal Save Changes with its own
 * revision (history keeps every state). Never touches handling, Flag for Report or Favorite.
 */
export async function changeFlags(
  e: T.Entry,
  patch: { important?: boolean; follow_up?: T.FollowUp },
  reason: string,
): Promise<T.Entry | null> {
  if (!app.active) return null;
  const f = formFromEntry(e);
  if (patch.important !== undefined) f.important = patch.important;
  if (patch.follow_up) f.follow_up = patch.follow_up;
  const input = toEntryInput(f, app.active.info.logbook_type, app.zone);
  input.event_timezone = e.event_timezone;
  input.event_utc_offset_min = e.event_utc_offset_min;
  try {
    const r = await api.saveChanges(e.id, e.current_revision, input, reason, newId(), null);
    app.saveStatus = {
      kind: "saved",
      seq: r.entry.sequence_no,
      at: r.entry.updated_at_utc,
      edit: true,
      duplicate: false,
    };
    app.announce(`Entry ${r.entry.sequence_no}: ${reason}`);
    app.bumpData();
    return r.entry;
  } catch (err) {
    const ae = toAppError(err);
    app.noteSaveFailure(ae);
    app.toast("error", `Not changed: ${ae.message}`);
    return null;
  }
}
