# Android tablet spike (Galaxy Tab S7 FE)

Status: **spike / test build**, 2026-11-03, branch `android-tablet-spike`. Not a supported platform yet.
Target: Samsung Galaxy Tab S7 FE only (12.4", 2560×1600, ≈1280×800 CSS px landscape at DPR 2, microSD,
optional Book Cover Keyboard). Phones are out of scope.

## TL;DR

- **The app runs.** `npx tauri android init` + a handful of `cfg(desktop)` / `cfg(mobile)` gates in
  `src-tauri/src/lib.rs` gives a working debug APK. `emcomm-core` (SQLite, migrations, lock, fsutil,
  backups, PDF/ICS exports) **compiled for Android with no changes**.
- **Durability holds.** Entry saved → `am force-stop` → relaunch: the entry is there, the stale lock
  triggers the integrity check ("not closed cleanly… Integrity check passed"), and the unfinished draft is
  offered for recovery and restores without being finalized.
- **What breaks:** folder/file dialogs (no real paths), native menus (don't exist), "Open/Reveal"
  (no `xdg-open`), the on-screen keyboard covers fields, edge-to-edge drawing under the status/nav bars,
  close/backup workflow never runs on Android lifecycle exits, and **a blank white screen after Back →
  relaunch** (activity recreated in a cached process). All are fixable; none touch the core's data model.
- **Not yet tested on the physical tablet.** Everything below is from a tablet-shaped emulator. The user
  will sideload the APK; see "Sideload the test build".

## Environment

| Item | Value |
|---|---|
| Host | macOS 14.6.1, Apple Silicon |
| Toolchain | Android SDK (platforms 34/35/36), NDK 27.2.12479018, OpenJDK 17 (Homebrew), Rust targets `aarch64/armv7/i686/x86_64-linux-android`, Tauri CLI 2.11.5 |
| Generated project | `src-tauri/gen/android` (compileSdk/targetSdk 36, minSdk 24, `applicationId org.emcommlog.desktop`) |
| Emulator AVDs | `TabS7FE_API34` (Android 14, AOSP image, WebView 113) and `TabS7FE_API35` (Android 15, Google APIs image, WebView 124); both 2560×1600 @ 320 dpi, landscape, hardware keyboard, 512 MB SD card |
| WebView viewport | 1280×800 CSS px, `devicePixelRatio` 2 — the same as the Tab S7 FE |

The Tab S7 FE's last major update is Android 14 (One UI 6), so the API 34 image is the closer OS match;
API 35 was used for most testing because its newer WebView avoids an emulator rendering artifact (below).
The real tablet updates its WebView through the Play Store, so it will be newer than either.

### Build commands

```bash
export JAVA_HOME=/opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home
export ANDROID_HOME=~/Library/Android/sdk NDK_HOME=~/Library/Android/sdk/ndk/27.2.12479018
npx tauri android build --debug --apk --target aarch64
# -> src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
```

The debug APK is 222 MB because the Rust library is unstripped debug code (214 MB `.so`); a release
build will be a small fraction of that.

### Sideload the test build

The built APK is at `release/android/emcomm-log-2.0.0-android-spike-debug.apk` (not committed;
SHA-256 `d7ff52936c1c25c5256f3673ed88c01c0968174c0e5c376a9ac14baad2d62027`). It is arm64-only,
debug-signed, and logs to logcat.

1. Copy the APK to the tablet (USB file transfer, or a microSD card).
2. On the tablet open it from **My Files**; allow "Install unknown apps" for My Files when asked.
3. Or with USB debugging on: `adb install -r release/android/emcomm-log-2.0.0-android-spike-debug.apk`.

What to look at on the real tablet (things the emulator could not settle): the tile-repaint artifact,
Samsung Keyboard behaviour over dialogs, the Book Cover Keyboard (Ctrl+Enter, Tab, Ctrl+1–5), S Pen
taps, DeX mode, and whether Samsung's battery manager kills the app in the background.

## Code changes in this spike

All in `src-tauri/src/lib.rs`, gated so desktop builds are byte-for-byte the same behaviour
(`cargo clippy --workspace --all-targets -D warnings` and `cargo fmt --check` clean on macOS; clippy
`-D warnings` also clean for `aarch64-linux-android`):

| Change | Why |
|---|---|
| `mod menu` and the menu setup (`set_menu`, `on_menu_event`) under `#[cfg(desktop)]` | `tauri::menu` doesn't exist on mobile (compile error E0432/E0599). |
| `fit_window_to_screen` under `#[cfg(desktop)]` | Mobile windows are always full-screen. |
| `FileDialogBuilder::set_parent` under `#[cfg(desktop)]` | Desktop-only API. |
| `pick_folder` on mobile → fixed app-private folder (`<app data>/logbooks`, or `/backups` when the title mentions backup) | `blocking_pick_folder` doesn't exist on mobile, and Android's folder picker returns a `content://` tree URI, not a path SQLite can open. |
| `pick_save_file` on mobile → `<app data>/exports/<name>` | Same reason; a stand-in until a share sheet / "create document" flow exists. |

`pick_file` (Import package, Attach file) was left on the dialog plugin to see what it does on Android.
The generated `src-tauri/gen/android` project is committed so the build can be reproduced.

## Results

### What worked

| Area | Result | Evidence |
|---|---|---|
| Launch, first-run wizard | Renders at 1280×800, all 6 steps complete; timezone detected (America/New_York). | `android-spike/01-wizard.png` |
| Storage in app-private dir | `app_data_dir()` = `/data/user/0/org.emcommlog.desktop` (ext4, internal). The wizard's write probe passes; `classify_location` → Local. | `03-storage-app-private.png` |
| Create logbook | Emergency logbook created; on disk exactly as on desktop: `logbook.sqlite` + `-wal`/`-shm`, `attachments/`, `logbook.json`, `.emcomm.lock`. | `run-as … ls` |
| Save entry | Quick Entry with the hardware keyboard (Tab between fields, **Ctrl+Enter** to save) → "Entry Saved #1". | `07-entry-saved-draft-typed.png` |
| Drafts | Draft JSON written to `<app data>/drafts/<logbook>/…json` ~2 s after typing stopped. | |
| Automatic backup | The 15-minute scheduler made its first verified backup (`backups/<book>/…-auto/` with manifest). | |
| **Force-stop durability** | `am force-stop` with a saved entry and an unsaved draft. The DB pulled off the device afterwards held entry #1 with the exact text and no second entry. Relaunch → open logbook → "Recover unfinished entries" listed the draft with its time; **Restore** put back From/Summary/Details exactly; the logbook still had 1 active entry. A toast reported "not closed cleanly last time. Integrity check passed." | `08-after-force-stop-recovery.png` |
| Logbook lock (`core/src/logbook.rs`) | `File::try_lock` (flock) works on Android. The lock file kept the dead PID after force-stop and the stale-lock integrity check ran on reopen, the same as desktop. No changes needed. | |
| fsutil (`core/src/fsutil.rs`) | Compiles unchanged. `target_os = "android"` is neither `linux` nor `macos`, so `is_network_fs` falls to the always-`false` stub, which is right for app-private storage. `sync_dir`/`atomic_write` use the `unix` paths and work. macOS-only `fullfsync` is skipped; plain `fsync` on ext4/f2fs is the normal Android guarantee. | |
| PDF export | Report PDF (384 KB, embedded DejaVu) written to `<app data>/exports/`. | `10-pdf-export-app-private.png` |
| Rotation | Rotating to portrait does **not** restart the activity (Tauri's manifest declares `configChanges`), so there is no state loss on rotation. | |
| Portrait | At 800×1280 CSS px every screen tried still worked (sidebar + content), below the desktop 960 px minimum. The Traffic Log loses its Summary and Status columns. | `11-portrait-reports.png`, `12-portrait-traffic-log.png` |
| Offline | Spot check after launch: the page loaded only from `http://tauri.localhost`, and no TCP/UDP sockets belonged to the app's UID. (The manifest still requests `INTERNET`; see F12.) | |

### What failed or needs work

| # | Problem | Detail | Severity |
|---|---|---|---|
| F1 | **Blank white screen after Back → relaunch** | Back finishes the activity but Android keeps the process cached. Relaunching creates a new activity in the same process; Tauri's `setup` (which creates the window, because `tauri.conf.json` has `"create": false`) does not run again, so the old WebView stays detached (`attached:false`) and the screen is white until the process is killed. | **Blocker** |
| F2 | **Close workflow never runs** | Back, Home, swipe-away and force-stop never send `CloseRequested`/`ExitRequested`. No close backup, no WAL checkpoint, and the lock stays held (harmless: the next open runs the integrity check). Saved data is safe (A03/A06 behaviour above); only the "backup on close" promise is broken. | High |
| F3 | **Folder picker** | No path-returning folder picker exists on Android. Spike shim: fixed app-private folders. "Open Folder…", "Relink" and "Restore into which folder?" all resolve to the same fixed folder, which is wrong for the real app. | High (design) |
| F4 | **File picker returns nothing** | `pick_file` opens the system picker (`GET_CONTENT` → DocumentsUI, `09-android-file-picker.png`), but the result is a `content://` URI. `FilePath::into_path()` fails, the command returns `None`, and the Import dialog stays empty with **no error**. Import package and Attach file are unusable. | High |
| F5 | **Open / Reveal** | `os_open` runs `xdg-open`, which doesn't exist: "Could not open the file manager: Permission denied (os error 13)". Exports exist only in app-private storage, where no other app can reach them. | High |
| F6 | **On-screen keyboard covers fields** | Samsung/Gboard covers ~45 % of the screen. The WebView is not resized and `visualViewport.height` stays 800, so the focused field and dialog buttons are hidden behind the keyboard (`02-onscreen-keyboard-covers-form.png`, `06-gboard-covers-dialog.png`). With a hardware keyboard this doesn't happen. | High |
| F7 | **Edge-to-edge overlap** | The generated `MainActivity` calls `enableEdgeToEdge()` (required at targetSdk 35+). The 60 px top bar draws under the status bar (clock over the logo) and the bottom logbook switcher and toasts draw under the gesture bar. | Medium |
| F8 | **Native menus** | No menus on Android. Menu items go through the same `runAction` ids as the sidebar and shortcuts, so most have an on-screen control. **Menu-only on a touch device:** Help → Keyboard shortcuts (F1 or `/` needs a keyboard), Help → Where is my data, Help → About, and the theme toggle. Quit isn't needed on Android. Shortcuts work with a hardware keyboard (Ctrl+Enter tested). | Medium |
| F9 | **Touch text selection** | A long press (and sometimes a slow tap) on nav labels and dialog text selects the text and shows Copy/Share/Select all (`13-touch-text-selection.png`). App chrome needs `user-select: none` and `-webkit-touch-callout: none`. | Low |
| F10 | **Wording** | "Everything stays on this computer", "Your computer's zone", "Nothing on this computer is overwritten" — needs device-neutral wording. | Low |
| F11 | **Stale entry count after a kill** | The logbook list showed "Entries 0" after a force-stop, before reopening (the index count updates on close). The same happens on desktop after `kill -9`; it matters more on Android because a clean close (F2) is rare. | Low |
| F12 | **Generated manifest** | Asks for `android.permission.INTERNET` (needed only for `tauri android dev`'s dev server) and declares Android TV (leanback). Both contradict "fully offline, tablet only" and should be removed for release. The package id `org.emcommlog.desktop` comes from the desktop identifier. | Medium (release) |
| E1 | *Emulator only?* Tile repaint artifact | On both emulator images (WebView 113 and 124, host GPU and SwiftShader), after the wizard a horizontal band of the screen showed a copy of the top of the page (`04-emulator-tile-artifact.png`). The WebView's own frame capture (`05-logbooks-webview-frame.png`) is correct and the DOM layout is correct; the band follows a raster-tile row and it went away later in the session. It's probably the emulator's GPU path. **Check on the real tablet.** | Unknown |

## Proposed phased plan for the full tablet port

Each phase ends with a sideloaded build on the Tab S7 FE, checked by hand. Desktop behaviour must not
change; every Android path stays behind `cfg(target_os = "android")` / `cfg(mobile)` or a capability flag
from `get_bootstrap`.

### Phase A — Lifecycle and shell (fixes F1, F2, F7, F12)

- Fix the white screen: on mobile, let Tauri create the main window from config (`"create": true` for
  Android via a platform config overlay, `tauri.android.conf.json`) instead of `create_main_window`, or
  re-create the webview when the activity is recreated. Test Back → relaunch, Recents swipe → relaunch,
  "Don't keep activities" developer option, and low-memory kill.
- Lifecycle hooks: a small Kotlin plugin (or Tauri's mobile `RunEvent`s where available) forwards
  `onPause`/`onStop` to Rust. On `onStop`: flush the draft and run a **"background" backup** (a new kind,
  or reuse "close") under a `WorkManager`/short foreground task so Android doesn't kill it mid-write. The
  backup code already writes to a temp folder and publishes it with an atomic rename, so a kill mid-backup
  leaves nothing partial; keep it that way. Checkpoint the WAL in `onStop`. Treat process death as normal:
  the stale-lock integrity check already handles it.
- Insets: draw inside safe areas — `env(safe-area-inset-*)` padding on the top bar, sidebar footer and
  toasts (needs `viewport-fit=cover`), or opt out of edge-to-edge for the activity.
- Manifest: remove `INTERNET` from release builds (keep it only for `tauri android dev`), remove leanback,
  add `android:resizeableActivity` / large-screen declarations, set `minSdk` to 30 (the tablet runs 14),
  and choose the package id (`org.emcommlog.tablet` or `org.emcommlog`; it can't change after the first
  install without losing data).
- Ask Samsung's battery optimisation to leave the app alone (with an explanation shown in Settings) so the
  15-minute backup timer runs while the app is in the foreground and is not frozen.

### Phase B — Storage, file picker, share sheet, microSD (fixes F3, F4, F5)

- **Live logbooks stay in app-private internal storage** (`<app data>/logbooks/<name>--<id>/`). It's the only
  place with a real path, flock, and rename guarantees. The UI shows "On this tablet" instead of a path
  and offers no folder choice. "Open Folder…"/"Relink" are hidden on Android. Location checks (inode,
  folder present) keep working unchanged.
- **Import / attach (open):** add a mobile `pick_file` that copies the `content://` URI's stream into
  `<cache>/incoming/` (via the dialog plugin's file handle or a small Kotlin helper) and returns that
  path. The existing `.emlog` validation then runs on a real file. Attachments go the same way into
  `stage_attachment`.
- **Exports and `.emlog` (save):** write to `<cache>/exports/` as today, then hand the file to the user
  with **SAF `ACTION_CREATE_DOCUMENT`** ("Save to…", which reaches Downloads, the SD card, USB drives)
  and an **Android share sheet** (`ACTION_SEND` through the existing `FileProvider`) for "Open"/"Send".
  Replace `os_open`/`reveal_path` on Android with "Share" / "Open with…" intents.
- **Backups, two tiers:**
  1. Automatic backups default to app-private internal storage (always available, survives SD card
     removal, but is lost on uninstall and on the same flash chip).
  2. **microSD backups:** `Context.getExternalFilesDirs()` gives a real, writable path on the SD card
     (`/storage/XXXX-XXXX/Android/data/<pkg>/files`) with no permission needed. The existing backup
     code (temp folder, verify, rename, retention) can run there unchanged. Caveats to show in the UI:
     Android deletes that folder on uninstall, and the card can be removed (the destination-missing case
     already exists, A08). For copies that must survive an uninstall, offer **"Copy latest backup to…"**
     through SAF (`ACTION_OPEN_DOCUMENT_TREE` on the SD card root, persisted URI permission), writing an
     `.emlog` package. Detect SD mount/unmount and show it in the storage health indicator.
- Restore and Import always create the new logbook in internal storage; the "into which folder?" step goes
  away on Android.

### Phase C — Touch and on-screen keyboard (fixes F6, F9)

- Keyboard: set `windowSoftInputMode="adjustResize"` and, because edge-to-edge apps get no automatic resize,
  apply the IME inset to the WebView container in `MainActivity` (`ViewCompat.setOnApplyWindowInsetsListener`
  with `WindowInsetsCompat.Type.ime()`). The fallback is the WebView's `navigator.virtualKeyboard`
  (present on this WebView) with `overlaysContent` and the `keyboard-inset-height` CSS env var. Which
  of the two works has to be tried on the tablet. Scroll the focused field into view and keep the dialog
  footer (Save/Cancel) above the keyboard. Test with Samsung Keyboard and the Book Cover Keyboard (which
  should hide the on-screen keyboard).
- Touch targets ≥ 44 px (sidebar items already are; check table rows, the star, icon buttons in the
  logbook list, checkboxes, date/time pickers). `user-select: none` and `touch-action: manipulation` on
  chrome; keep selection inside text fields and entry bodies.
- Hover-only cues (tooltips, row actions) need tap equivalents. Long press on a Traffic Log row → context
  actions, replacing right-click.
- Show shortcut hints ("Ctrl+Enter") only when a hardware keyboard is attached
  (`matchMedia("(pointer: fine)")` or the Android keyboard configuration).
- Device-neutral wording (F10).

### Phase D — Orientation decision

- Recommendation: **support both orientations, landscape first.** Portrait (800 CSS px) already works for
  everything except wide tables. Tauri's manifest avoids restarts on rotation, so rotation is safe for
  in-progress entries.
- In portrait: the Traffic Log collapses to two-line rows (time/station/type, then summary/status),
  the sidebar becomes icons only, and entry detail opens as a full-height sheet.
- If portrait turns out to be poor in practice, lock to `sensorLandscape` in the manifest (one line)
  rather than maintain two layouts.

### Phase E — Signed APK packaging

- Create an upload/release keystore (kept outside the repo, with an offline backup). Configure
  `signingConfigs` in `gen/android/app/build.gradle.kts` from `keystore.properties` (git-ignored).
- `npx tauri android build --apk --target aarch64` (release, arm64-only: the Tab S7 FE is arm64), with
  stripped symbols. Publish the APK next to its SHA-256, the same way as the desktop builds.
- `versionCode` from `tauri.conf.json`'s version; keep the same signing key forever (Android refuses an
  update signed with a different key, and uninstalling deletes app-private logbooks; see Phase B).
- Distribution is sideloading (no Play Store, no accounts). Document "Install unknown apps" and the
  upgrade path, and add a one-click "Export everything before uninstalling" in Settings.
- Optional: a CI job builds the APK on `ubuntu-22.04` with the NDK, so Android builds don't depend on
  this Mac.

### Phase F — Verification on the tablet

Re-run the acceptance items that matter on a tablet, on the device: A03 (force-stop and
reboot with a saved entry), A05 (SD card removed mid-backup, storage full), A06 (draft recovery after
swipe-away, low-memory kill, reboot), A08 (backups: automatic, background and microSD; retention),
A10 (`.emlog` from the Mac imported on the tablet and back), A11 and the ICS forms opened in a PDF viewer on
the tablet, A14 (Book Cover Keyboard shortcuts, TalkBack pass). Add an `adb`-driven smoke test,
modelled on `scripts/mac_smoke.sh`: install, launch offline, wizard rendered, data store created.

## Files

- `docs/android-spike/*.png` — screenshots from the emulator. 01 and 02 are from the API 34 image (WebView
  113); the rest are from API 35 (WebView 124). Files named "webview-frame" were captured through
  WebView remote debugging; the others are device screenshots.
- `src-tauri/gen/android/` — the generated Android Studio / Gradle project.
- `release/android/` — the local debug APK (git-ignored).
