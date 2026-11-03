import { app } from "./state.svelte";
import { api, toAppError } from "./api";
import { confirm } from "./confirm.svelte";
import type * as T from "./types";

/**
 * Put a recovered draft back into an editor. Nothing is saved until the operator chooses
 * Save Entry / Save Changes, so a recovered draft never silently finalizes or overwrites.
 */
export async function restoreDraft(d: T.Draft): Promise<boolean> {
  if (d.editor === "new") {
    app.editorRequest = { mode: "draft", draft: d };
    await app.navigate("quick");
    return true;
  }
  if (!d.entry_id) return false;
  let entry: T.Entry;
  try {
    entry = await api.getEntry(d.entry_id);
  } catch (e) {
    app.toast("error", `The entry for this draft could not be loaded: ${toAppError(e).message}`);
    return false;
  }
  if (entry.status === "void") {
    app.toast("warn", `Entry #${entry.sequence_no} is voided; restore it before applying this draft.`);
    return false;
  }
  if (d.base_revision !== null && entry.current_revision !== d.base_revision) {
    const a = await confirm.ask({
      title: "Entry changed since this draft",
      message: `Entry #${entry.sequence_no} is now at revision ${entry.current_revision}; the draft was based on revision ${d.base_revision}.`,
      detail:
        "If you continue, the draft's text is loaded into the editor on top of the latest revision. Review it before saving.",
      buttons: [
        { id: "load", label: "Load draft anyway", tone: "primary" },
        { id: "cancel", label: "Cancel" },
      ],
      cancelId: "cancel",
    });
    if (a.id !== "load") return false;
  }
  app.editDraft = d;
  app.detailEntryId = entry.id;
  return true;
}
