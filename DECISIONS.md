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
