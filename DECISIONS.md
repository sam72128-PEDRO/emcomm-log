# Decisions and deviations

Ambiguities in the specification and mockups, and the choice made. Data integrity wins ties.

1. **Mockup controls not in the spec.** The mockups show *Official Report* flags, message status
   (pending / awaiting ACK / acknowledged), *Messages*, *Search*, and *Operational Notes*. Rather than omit
   them, each is a real, stored feature: `flagged` (boolean, exported, filterable), `handling` (enum on any
   entry, changed via Save Changes so it gets a revision), Messages screen (entries with a handling state),
   Search (focuses Traffic Log search, same as Ctrl/Cmd+F), Operational Notes (logbook setting, travels in
   `.emlog`). Mockup "Type" badges (MSG, INFO, TX, RELAY…) map to entry *kind* (traffic, message, status,
   note, qso, activity) plus *direction* (RX/TX/INT); INFO/RELAY/etc. are offered as preset quick tags.
2. **Mockup priority labels** "Normal/High" are shown as the spec's routine/priority/emergency ("Normal"
   displays for routine in the Traffic Log, as in the mockup).
3. **Window chrome / decorative art** from the mockups (custom min/max/close buttons, radio-tower artwork,
   storm photo) are not reproduced; native window decorations are used and the header uses a gradient.
4. **Sequence numbers** are allocated from `logbook.next_sequence` inside a `BEGIN IMMEDIATE` transaction
   with `UNIQUE(logbook_id, sequence_no)`; rolled-back attempts consume nothing. Entries use an explicit
   `rid INTEGER PRIMARY KEY` so `VACUUM INTO` snapshots keep FTS row ids stable.
5. **Idempotency**: Save Entry uses `entries.client_request_id`; edits/void/restore use
   `entry_revisions.client_request_id`. The UI reuses a request id only for an identical retry; changing
   the form after a failure starts a new intent. Save Changes also carries the base revision; a stale base
   is rejected (Conflict) instead of overwriting.
6. **Unchanged Save Changes** writes nothing and adds no revision.
7. **Voided entries cannot be edited** until restored (keeps the timeline unambiguous).
8. **Live-location checks**: before every final write the app verifies the folder, database file identity
   (device/inode on Unix) and attachments folder; a mismatch blocks final saves until "Recheck storage".
   Network file systems and known cloud-sync folders (iCloud, Dropbox, OneDrive, Google Drive, Box) are
   **rejected** for live logbooks; backups may go to them with a warning.
9. **Exclusive lock** is an OS file lock on `.emcomm.lock`. Lock content left behind by a crash triggers a
   full `integrity_check` + `foreign_key_check` before the book opens (OS locks are released when the owner
   dies, so a lock we can acquire has no active owner).
10. **macOS durability**: `PRAGMA fullfsync` and `checkpoint_fullfsync` are enabled in addition to
    `synchronous=FULL`, because plain `fsync` does not flush the drive cache on macOS.
11. **Retention**: in addition to 7 daily / 4 weekly, the newest **8** backups are always kept so
    intra-day restore points exist at a 15-minute interval. All three numbers are configurable.
    Automatic backups are skipped when nothing changed since the last verified backup (a content
    digest); close and manual backups always run.
12. **Backup format** is a directory (manifest + `logbook.sqlite` + `attachments/`) published by atomic
    rename after hash and integrity verification; `.emlog` is the ZIP form of the same layout.
13. **Restore identity**: the original UUID is kept unless it is already in the local index (spec). **Import**
    always assigns a new identity (spec) and records provenance; a package from a known logbook is
    reported as a duplicate but still imported as a separate copy, never merged.
14. **Package limits**: 8 GB archive, 16 GB expanded, 4 GB database, 512 MB per attachment, 200,000 members.
    Duplicate member names are detected from the ZIP central-directory count because the ZIP library
    collapses duplicates by name.
15. **CSV metadata**: a fixed 7-line preamble (title, logbook, type, generated, scope, records, blank) precedes
    the stable header row; parsers skip 7 lines. UTF-8 with BOM for spreadsheet compatibility. Formula
    escape: a leading apostrophe on cells starting with `= + - @ TAB CR`.
16. **XLSX** stores all values as text cells (never formulas); formula-like text additionally gets Excel's
    `quotePrefix` style. The event time is also written as a typed UTC datetime column. Text over 32,000
    characters (Excel's cell limit) is truncated **in XLSX only**, with a visible marker and an export
    warning; TXT/DOCX/PDF/CSV carry full text.
17. **PDF** embeds DejaVu Sans (full font, no subsetting, ~400 KB per file) with a ToUnicode map. DejaVu has
    no CJK glyphs: CJK text renders as boxes in PDF (still extractable via ToUnicode for most text and fully
    present in the other formats). Bold is synthesized with text render mode 2.
18. **Exports are always chronological** by event time with sequence tie-break regardless of the Traffic
    Log's sort, and range scope dates are inclusive local (or UTC) dates converted to exact UTC bounds.
    For *current filter* scope, "Include voided" decides active vs. active+voided; a void-only filter stays
    void-only.
19. **ADIF**: version pinned to **3.1.4**. Eligible = active, has counterpart callsign, QSO start, and band or
    frequency. BAND may be derived from FREQ via the ADIF band table (not fabrication: the frequency is
    exported too). Unknown modes and non-ASCII comments are omitted with warnings, never transliterated.
    Common submodes (USB/LSB, FT4, JS8, PSK31, DMR, C4FM…) map to their parent MODE + SUBMODE.
20. **Templates/presets**: built-in presets per type (Emergency includes Shelter Update, Welfare Check,
    Resource Request, Situation Report — matching the mockup's quick-template buttons). They are copied into
    a new logbook as ordinary editable templates; nothing is seeded as entries (no demo data).
21. **Close/quit**: the window close and Quit route through the UI, which flushes the draft, asks if unsaved
    text remains, runs the close backup, and asks Retry / Close without backup / Stay if it fails. A
    second close request 3–15 s later forces exit in case the UI is unresponsive (drafts are already on disk).
22. **Webview capabilities** are limited to event listen/unlisten. File dialogs run in Rust, so JavaScript
    never receives filesystem permissions; revealing/opening paths is restricted to logbook, backup,
    app-data, and this session's export locations.
23. **`EMCOMM_DATA_DIR`** environment variable relocates app data (testing/portable). Not shown in the UI.
24. **Emergency entries** snapshot the logbook's incident name/number/operational period at save time when
    not entered, so later changes to the logbook's operational period don't rewrite history.

## V2 decisions

25. **V2 is developed on macOS only** (V2 spec §1). Windows/Linux CI runs on manual dispatch until V2
    Phase 5; code stays cross-platform.
26. **ICS forms are FEMA's own fillable PDFs** (Phase 1 gate, passed 2026-11-03): ICS 213 v3 and ICS 214
    v3.1 are AcroForm (no XFA), unencrypted, bundled in `core/assets/ics/` with hashes in
    `docs/ICS_FIELD_MAP.md`. Filled with `lopdf` 0.46: EMCOMM LOG writes each field's value and its own
    appearance stream in the embedded DejaVu Sans (the forms' Helvetica cannot show ✓ or many accented
    names), and sets the field's default appearance to that font so it stays editable. Verified on this Mac
    in macOS Preview (renders, edits in place, recognised as a form) and Chrome's PDF viewer (renders,
    fields editable); Acrobat Reader and Firefox are not installed here and are checked at Phase 5.
    Instruction pages are dropped; signature fields are never filled; text that does not fit is returned
    as overflow for a continuation page, never silently cut.
27. **ICS 309 is drawn by EMCOMM LOG**, not filled: FEMA's NIMS v3 set has no ICS 309, and online copies
    are older revisions or local ARES variants. `export::ics309` draws the standard layout (blocks 1–8) with
    the same embedded font; cells wrap rather than truncate. A message number goes under FROM unless the
    entry was sent (then TO). Being drawn, the 309 is not a fillable form.
28. **No ICS 205 export.** The channel plan is entered from the 205 an operator is given (input only).
    Schema v3 adds `channel_plan` (the 205 channel table, one row per channel) and the period's special
    instructions. A chosen channel is stored in the entry as plain text ("TAC 2 (155.4750 MHz)"), not as a
    reference, so editing a plan never rewrites history. Copying a plan from another logbook opens that
    database read-only through the logbook index.

## 2.1 decisions (desktop GPS, Quick Entry, HAM, operator spec EMLOG-DESKTOP-ENH-2026-11-03)

29. **Station and operator are separate records.** Schema v4 adds a one-row `station` table per logbook
    (tactical ID, label, licensed station callsign, agency, default position, activation refs) and
    `operator_sessions` (shift history). Each entry stores a `station_snapshot_json` beside the existing
    operator snapshot, so later edits to the station or a profile never change history. HAM
    `STATION_CALLSIGN` is prefilled from the station, never the operator's personal call.
30. **Switch Operator is one instant, no period change.** The handover time may be now or earlier within the
    current shift (not in the future, not before the shift began, not to the person already on duty). It does
    not touch operational periods or ICS forms. An unsaved entry at handover is never silently reassigned: the
    operator chooses *save it now under the outgoing operator* (blocked until it saves) or *leave it for the
    incoming operator*, who then becomes its recorded operator. Profiles stay global and local-only (no
    profile export).
31. **Locations are JSON value objects on the entry**, not rows in a shared table: `operator_location_json`
    and `reported_location_json` each hold coordinates (WGS84, both or neither), label, grid, source, provider
    id, fix time, accuracy, altitude, satellites, notes. Copying a value makes it immutable history with no
    joins; *Use default* copies the station default and records the original source in `provider_id`.
32. **Own NMEA parser, not the `nmea` crate**: only RMC/GGA/GSA are needed and the acceptance rules (checksum
    mandatory; RMC status, mode and GGA quality must agree) are stricter than a general parser's. Serial via
    `serialport` 4.10 with default features off (no libudev dependency on Linux). Nothing is ever written to
    the receiver. Baud is probed at 4800 then 9600 unless fixed. Accuracy is never derived from HDOP.
33. **OS location only through native APIs, on explicit request.** macOS Core Location (objc2, main thread,
    `NSLocationWhenInUseUsageDescription`), Windows `Geolocator` (satellite-sourced → *internal GNSS*, otherwise
    *OS position*), Linux GeoClue2 over D-Bus (zbus, blocking). The Tauri geolocation plugin is mobile-only and
    is not used on desktop. gpsd is accepted on loopback addresses only.
34. **Export privacy default is off.** Shared formats (CSV/XLSX/DOCX/TXT/PDF/ADIF) withhold latitude/longitude
    and cut grids to 6 characters unless *Include precise coordinates* is ticked per export; ADIF then writes
    `MY_LAT`/`MY_LON` from the station position only. `.emlog` packages and backups are full copies.
35. **ADIF 3.1.8, one writer, four presets.** A preset changes only eligibility and, for POTA, adds
    `MY_SIG`/`MY_SIG_INFO` (and `SIG`/`SIG_INFO` park-to-park) as POTA's uploader expects; `*_POTA_REF` and
    `*_SOTA_REF` are written whenever present. Skipped QSOs are listed with reasons and require an
    acknowledgement. No SOTA CSV (the spec allows it only against a validated SOTAData layout) and no upload.
36. **Archived logbooks are not modified by viewing.** An archived logbook at an older schema opens from a
    `VACUUM INTO` temporary copy that is migrated; the original is upgraded (with the usual verified
    pre-migration copy) only when unarchived.
37. **Recents are a query, not a table**: the newest 2000 active entries, grouped per field, newest first then
    most used, 30 values, offered through `<datalist>` so a value is applied only when picked.

## Android tablet decisions

38. **Tablet only, Android 11+ (API 30), arm64.** The manifest declares large/xlarge screens only
    (`requiresSmallestWidthDp=600`). Package id `org.emcommlog.tablet` via `tauri.android.conf.json`,
    so the desktop identifier and its app-data folder are unchanged.
39. **Live logbooks live in app-private internal storage.** Android gives apps real paths, file locks
    and atomic renames only there; SAF `content://` documents cannot back a live SQLite database. The
    lock, location checks and WAL/`synchronous=FULL` behave as on desktop (`fsutil` treats Android as
    "not a network file system"; no code change needed).
40. **Backups go to the microSD card's app-specific folder** (`getExternalFilesDirs`) or to internal
    storage. That folder is a real path, so the existing verified backup code runs unchanged. Android
    deletes it on uninstall, so copies that must outlive the app are `.emlog` packages saved through SAF.
41. **Files in and out go through the app cache.** Picked documents are copied into the cache before
    the core reads them; exports are written to the cache (temp + rename as on desktop) and then copied
    to the document chosen with "Save to…". Only `cache/shared/` is exposed through the FileProvider.
42. **No INTERNET permission and `allowBackup=false`:** the app cannot use the network, and Android cloud
    backup never copies its data.
43. **Android lifecycle.** Leaving the foreground flushes drafts, checkpoints the WAL and runs an
    automatic backup (skipped if unchanged), because Android may end the process without a close event.
    When the activity finishes, the process ends, so a launch never reuses a process without a webview.
    The last open logbook is reopened on launch (remembered per device in WebView storage).
44. **Back button** closes the top dialog, then returns to the Dashboard/Logbooks screen, then offers
    Exit, which runs the normal close workflow. Text inputs keep the system's own Back (hide keyboard).
45. **Release signing key** is kept outside the repository (`~/.emcomm-android/`), referenced by a
    git-ignored `keystore.properties`.
46. **Tablet location instead of external GPS (2.1 on Android).** External receivers (serial NMEA, gpsd)
    are not used on the tablet; the core already leaves `serialport` out of Android builds. The 2.1 "OS
    location" source maps to Android's `LocationManager.getCurrentLocation`: the GPS provider when the
    tablet has one (works offline → *internal GNSS* / Device GPS), otherwise the network provider
    (*OS position*). No Google Play services, no tracking: one fix per explicit Capture/Test, the
    permission requested only then, the desktop freshness rule (`gps::os::outcome`) applied, a
    satellite count of 0 treated as not reported, and a 90 s timeout for cold GPS starts.
