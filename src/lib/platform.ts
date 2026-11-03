// Platform facts the UI needs before (or without) the bootstrap. Android runs in a system
// WebView whose user agent always names Android; tests (jsdom) and desktop builds do not.
export const IS_ANDROID = typeof navigator !== "undefined" && /Android/i.test(navigator.userAgent);

/** "tablet" on Android, "computer" elsewhere — for wording such as "stored on this tablet". */
export const DEVICE = IS_ANDROID ? "tablet" : "computer";

/** Android exports go through "Save to…"; show the file name rather than the app-cache path. */
export function shownPath(p: string): string {
  if (!IS_ANDROID) return p;
  const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
  return i >= 0 ? p.slice(i + 1) : p;
}
