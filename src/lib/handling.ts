import { app } from "./state.svelte";
import { api, newId, toAppError } from "./api";
import type * as T from "./types";
import { formFromEntry, HANDLING_LABEL, toEntryInput } from "./entryForm";

/** Change only the handling status of an entry, as a normal Save Changes with its own revision. */
export async function changeHandling(e: T.Entry, h: T.Handling | null): Promise<boolean> {
  if (!app.active) return false;
  const f = formFromEntry(e);
  f.handling = h ?? "";
  const input = toEntryInput(f, app.active.info.logbook_type, app.zone);
  input.event_timezone = e.event_timezone;
  input.event_utc_offset_min = e.event_utc_offset_min;
  const label = h ? HANDLING_LABEL[h] : "not tracked";
  try {
    const r = await api.saveChanges(
      e.id,
      e.current_revision,
      input,
      `Handling changed to ${label}`,
      newId(),
      null,
    );
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
