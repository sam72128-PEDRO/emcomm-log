// 2.2 related communications: how a link reads from each end.
import type * as T from "./types";

/** [as seen from the source, as seen from the target] */
const WORDS: Record<T.Relation, [string, string]> = {
  related_to: ["Related to", "Related to"],
  follow_up_to: ["Follow-up to", "Followed up by"],
  relay_of: ["Relay of", "Relayed as"],
  response_to: ["Response to", "Answered by"],
  corrects: ["Corrects", "Corrected by"],
};

export function linkLabel(v: T.LinkView): string {
  return WORDS[v.link.relation][v.outgoing ? 0 : 1];
}

/** Choices when linking from the open entry: "this entry is a … of #n". */
export const RELATION_CHOICES: { value: T.Relation; label: string; hint: string }[] = [
  { value: "related_to", label: "Related to", hint: "Same matter; no direction" },
  { value: "response_to", label: "Response to", hint: "This entry answers the other" },
  { value: "follow_up_to", label: "Follow-up to", hint: "This entry follows up the other" },
  { value: "relay_of", label: "Relay of", hint: "This entry passes the other on" },
  {
    value: "corrects",
    label: "Corrects",
    hint: "A new message correcting an earlier one (to fix a typo, Edit instead)",
  },
];
