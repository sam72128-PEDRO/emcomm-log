# EMCOMM LOG on an Android tablet

EMCOMM LOG 2.1.0 runs on large Android tablets (Android 11 or later, arm64). It was built for and
tested against a Samsung Galaxy Tab S7 FE profile: 12.4", 2560×1600, about 1280×800 CSS px in landscape,
microSD slot, optional Book Cover Keyboard. Phones are not supported; the APK declares large screens only.

Same app, same core: the Rust `emcomm-core` crate (SQLite with WAL + `synchronous=FULL`, logbook
lock, drafts, verified backups, `.emlog` packages, exports, ICS 213/214/309) is unchanged and compiled
for Android. Everything is offline: the APK requests **no network permission** at all. Its only permission is
location, which Android asks for the first time you press **Capture GPS…** (see below).

All of 2.1 is on the tablet — station and operator, Switch Operator and shifts, two locations per entry,
export privacy, ADIF 3.1.8 with QRZ/POTA/SOTA presets, Quick Entry suggestions and Save & Add Another,
archived-logbook upgrades — except **external GPS receivers** (USB/Bluetooth NMEA, gpsd), which are not
used on the tablet.

## Install

1. Copy `EMCOMM-LOG-2.1.0-tablet.apk` to the tablet (USB cable, microSD card, or Quick Share).
2. Open it from **My Files**. Android asks once to allow **Install unknown apps** for My Files; allow
   it, then tap **Install**.
3. Open **EMCOMM LOG** from the app drawer and complete first-run setup.

Check the download against `EMCOMM-LOG-2.1.0-tablet.apk.sha256` (`f8910f29b6dfba1337ff5f4fdf826677b1364c07edf69a74602c0c79d167d359`) if you like:
`shasum -a 256 EMCOMM-LOG-2.1.0-tablet.apk` on a Mac.

Updates are installed the same way, over the existing app; logbooks are kept. Installing 2.1.0 over 2.0.0
upgrades each logbook the first time it is opened (schema 3 → 4), keeping a verified copy of the old
file in its `.pre-migration` folder; archived logbooks are shown from an upgraded temporary copy and
upgraded only when unarchived. An update must be signed
with the same key (see *Signing* below), otherwise Android refuses it.

> Uninstalling EMCOMM LOG deletes its private storage — the live logbooks **and** the backups in its
> microSD folder. Export each logbook as a package (Reports → *Export .emlog package…* → save to
> Downloads or the SD card root) before uninstalling.

## What is different from the desktop app

| Area | On the tablet |
|---|---|
| Live logbooks | Kept in EMCOMM LOG's private storage on the tablet (`/data/user/0/org.emcommlog.tablet/logbooks`). Android gives apps no other place with real file paths and file locks, which SQLite needs. There is no folder to choose, and *Open Folder…* / *Relink…* are not shown. |
| Backups | Choose **microSD card** or **Tablet storage** (Settings → Backups → *Choose…*). The card folder is `Android/data/org.emcommlog.tablet/files/EMCOMM LOG Backups` on the card. The wizard picks the card when one is inserted. If the card is removed, backups report *destination is not available* and saving continues normally; re-insert it and back up again. |
| Location | **Capture GPS…** (Station dialog, entry Location section) and **Settings → Location & GPS → Test GPS** use the tablet's own location: its satellite GPS when it has one — works with no network, labelled *Device GPS* — otherwise Android's network location, labelled *OS position* (needs a connection, so usually nothing offline). Android asks for permission on first use; *Only this time* and *Don't allow* both work (Don't allow → typed coordinates and the station default still work). One fix per press, never tracking. Choose *Manual only* in Settings to never use it. A cold GPS start can take a minute or more outdoors; the wait is up to 90 s. |
| Background | When EMCOMM LOG leaves the screen (Home, another app, screen off), unfinished entries are saved as drafts, the database is checkpointed, and an automatic backup runs if anything changed. Android may end a background app at any time; nothing saved is lost. |
| Reopening | Launching the app reopens the logbook that was open last. If the app was ended in the background, its integrity check runs first and unfinished entries are offered for recovery. |
| Back button | Closes the open dialog or panel, then returns to the Dashboard, then asks **Exit EMCOMM LOG?** Exiting runs the normal close: unsaved-entry warning, close backup, clean unlock. |
| Exports, reports, ICS forms, packages | *Save…* opens Android's **Save to** screen (Downloads, the microSD card, a USB drive, Drive…). After saving, **Open** hands the file to a viewer (e.g. the PDF viewer) and **Share** opens the share sheet (Quick Share, Bluetooth, email, Print…). |
| Import package, attach file | Android's file picker; the chosen file is copied into the app before it is checked and read. |
| Restore backup | *Choose…* offers the microSD card or tablet backup folders; the restored logbook is created in tablet storage. |
| Keyboard | The on-screen keyboard pushes the page up so the field you are typing in, and dialog buttons, stay visible. With the Book Cover Keyboard all desktop shortcuts work (Ctrl+Enter save, Ctrl+1–5, Ctrl+F, F1…). |
| Menus | Android has no menu bar; everything is on screen. **Help** in the sidebar opens *Where is my data?*, with *Keyboard shortcuts* and *About*. |
| Orientation | Landscape and portrait both work, and rotating never loses what you are typing. In portrait the sidebar collapses to icons. |
| System backup | Android cloud backup is disabled for the app (`allowBackup=false`): no data leaves the tablet. |

## Building the APK (on the Mac)

Prerequisites: Node 22+, Rust with `aarch64-linux-android` target, Android SDK (platform 36,
build-tools), NDK 27.2.12479018, JDK 17.

```bash
rustup target add aarch64-linux-android
export JAVA_HOME=/opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home
export ANDROID_HOME=~/Library/Android/sdk NDK_HOME=~/Library/Android/sdk/ndk/27.2.12479018
npm ci
npx tauri android build --apk --target aarch64          # signed release
npx tauri android build --debug --apk --target aarch64  # debuggable (WebView inspection), debug key
```

Output: `src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release.apk`
(about 16 MB). `src-tauri/tauri.android.conf.json` sets the Android package id (`org.emcommlog.tablet`) and
minimum Android 11 without changing the desktop identifier.

### Signing

Release builds are signed when `src-tauri/gen/android/keystore.properties` (git-ignored) exists, or
`EMCOMM_KEYSTORE_PROPERTIES` points at one:

```properties
storeFile=/Users/<you>/.emcomm-android/emcomm-release.jks
storePassword=…
keyAlias=emcomm
keyPassword=…
```

The current key lives in `~/.emcomm-android/` on the build Mac (`emcomm-release.jks` and
`keystore.properties`, owner-only permissions). Certificate SHA-256:
`8d0242e66a6c8eaccf3dcdfda96a14b1b014b6ecb932cd6fd098fc59661908c5`.
**Back that folder up somewhere offline.** Losing it means future versions cannot be installed as
updates; the app would have to be uninstalled (after exporting packages) and reinstalled.

### Code layout

| Path | Role |
|---|---|
| `src-tauri/src/android.rs` | Rust side of the bridge: picker copies, "Save to…" delivery, open/share, SD discovery, system bars, exit. |
| `src-tauri/gen/android/app/src/main/java/org/emcommlog/tablet/EmcommPlugin.kt` | Kotlin plugin: Storage Access Framework, FileProvider, share sheet, removable storage, window insets (system bars + keyboard), Back button, lifecycle → web UI. |
| `src-tauri/gen/android/app/src/main/java/org/emcommlog/tablet/MainActivity.kt` | Edge-to-edge bars, lifecycle forwarding, ends the process when the activity finishes. |
| `src/lib/androidShell.ts`, `src/lib/platform.ts` | UI side: Back handling, background save/backup, reopen last logbook, wording, theme-matched bars. |
| `src-tauri/gen/android/app/src/main/AndroidManifest.xml` | No permissions, large screens only, `adjustResize`, `allowBackup=false`. |

Desktop builds are unaffected: Android code is behind `cfg(target_os = "android")` in Rust and
`IS_ANDROID` (WebView user agent) in the UI.

## Verification

See `docs/ANDROID_TABLET_SPIKE.md` for the original spike and the problem-by-problem fixes, and
`IMPLEMENTATION_STATUS.md` (Android section) for the checks run on the emulator. A run on the physical
Tab S7 FE is still to be done by the operator.
