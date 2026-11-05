// 2.2 Review Export helpers.
import type * as T from "./types";

/** The approved entries with the revisions that were reviewed; null when the scope was too big to list. */
export function reviewedRefs(review: T.ExportReview | null, checked: Set<string>): T.ReviewRef[] | null {
  if (!review || review.too_many) return null;
  return review.rows
    .filter((r) => r.eligible && checked.has(r.id))
    .map((r) => ({ id: r.id, revision: r.revision }));
}
