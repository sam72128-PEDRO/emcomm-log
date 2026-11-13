# Changelog

## 2.3.0 — 2026-11-13

Three refinements from the V2.3.0 specification, built on 2.2 without a second system for any of them.
**No database change:** the logbook stays at schema 5 and the app database at version 2, so 2.3 logbooks,
backups and `.emlog` packages open in 2.2 and 2.2 ones open in 2.3 unchanged. Built and tested on macOS only;
nothing in the shared core is macOS-specific.

**Operator handover summary**
- Switch operator… → **Handover Summary…** previews a summary before the switch: logbook/incident, station,
  outgoing and incoming operator, shift start and handover time, operational period; this shift's entries by
  type plus Important/Flagged/Priority ones; outstanding work at the station (not transmitted, transmitted
  with no acknowledgment recorded, awaiting acknowledgment, open and overdue follow-ups, Important entries),
  each item marked when it was inherited from an earlier shift; the note received at shift start and the
  handover note. **Copy Summary**, click any entry to open it, **Continue Operator Switch**, or
  **Cancel / Return to Log**. Optional: Switch operator works exactly as before without it, and a summary
  that cannot be generated never blocks the switch.
- Traffic Log shift-change rows have **Shift summary**: the earlier shift, **reconstructed from records** and
  labelled as such (summaries are not stored, so none is ever presented as an original snapshot).
- A preview reads only; it never writes to the logbook. Archived logbooks can be summarised; switching in
  them stays blocked.

**Backup health and Verify Latest Backup**
- Settings → Backups shows **Backup Health**: Verified, Not Configured, Unavailable, Attention Required,
  Verification Failed or Unknown, with destination and whether it is reachable, schedule, last attempted,
  last successful and last verified backup, backup age, last recorded error, and whether the newest changes
  are in a backup. The Dashboard shows the state with a link to the details.
- **Verify Latest Backup** (off the UI thread) checks the folder and manifest, the SHA-256 of every file,
  SQLite integrity, foreign keys, every attachment the database references, and completeness against the
  manifest, and lists each check with its result. It only reads: it never changes, prunes, restores or
  repairs a backup. A backup is shown as Verified only after this check of that same backup; the check made
  when a backup is created is shown separately ("checked when created"), so new backups show **Unknown**
  until verified.
- A scheduled backup skipped because nothing changed is shown as such, never as a failure.

**Message status (traffic lifecycle)**
- Messages: *Transmitted, no ACK recorded* (Sent) and *Awaiting ACK* are separate tabs (2.2 combined them),
  plus *Relayed / Delivered / Closed*. New columns: outstanding action, last status change (time and
  operator), open follow-up and related-entry count.
- Entry Details → **Message status**: current status, outstanding action, latest change and who recorded it,
  the acknowledgment (when, by whom, note), the full status history, and the follow-up shown separately.
- Moving a status back (for example Acknowledged → Sent, or clearing the status of a transmitted message)
  now needs a reason, so recorded progress is never silently overwritten; the earlier status stays in the
  history and the change is marked as a correction. Forward changes stay one click.
- Nothing acknowledges a message except an explicit status change: transmitting, linking, resolving a
  follow-up, flagging, switching shifts and reports leave it alone (now covered by tests).

**Fixed**
- Automatic backups were skipped as "unchanged" after changes that create no entry revision — adding or
  removing a related-entry link, editing the logbook directory, an ADIF import batch record, or a shift
  change. These now count as changes. (Existing 2.2 backups are unaffected; the next scheduled backup after
  upgrading may run once even with no new data.)
- The Dashboard called the last successful backup "verified"; it now says "checked when created", and new
  backup records read "Created and checked".

## 2.2.0 — 2026-11-04

Six additions from the V2.2 handoff package (EMLOG-2.2-SHARED-2026-11-04). Logbooks upgrade from 2.1
(schema 4 → 5) with a verified pre-migration copy; nothing is removed or renumbered. **A 2.2 logbook or
`.emlog` package cannot be opened by 2.1**; 2.1 refuses it as a newer format rather than losing the new data.

**Attachments**
- One Attachments box in the editor: Attach files… (several at once), drag and drop from Finder/Explorer/
  file manager, or paste a screenshot into its paste field. Files are copied into the logbook at once, with
  progress and Cancel and a warning above 25 MB.
- Captions; where the file came from (file, dropped, pasted); type detected from the content.
- Removing an attachment is a recorded change (Save Changes); the file is kept for history and backups.
- Entry Detail shows thumbnails (small re-encoded copies without EXIF), captions, Open and Save As.
  Programs and scripts are never opened, whatever they are named.

**Related entries**
- Link an entry to another: Related to, Response to, Follow-up to, Relay of, Corrects. One-hop list in
  Entry Detail, link search by number or text, counts in the Traffic Log. Links never change either entry
  and are kept (with who removed them) when removed.

**Important and follow-up**
- Important and Follow-up needed (with due time and note), separate from Flag for report and from message
  status. Resolve/Reopen from Entry Detail. Traffic Log filters; Dashboard Open Follow-ups card (with overdue
  count) opens the filtered log. Every change is a revision. No reminders or notifications are sent.

**Logbook directory**
- Per-logbook list of stations, tactical identifiers and organizations (Settings → Logbook Directory, or
  Save to Directory from an entry). Offered first while typing; "Use these" fills channel/frequency/mode only
  when you click it. Editing the directory never changes saved entries. Travels with the logbook.

**ADIF import (HAM logbooks)**
- Reports → Import ADIF…: preview every QSO with possible duplicates (call, UTC minute, band, mode, station
  and park/summit) and anything that cannot be imported with the reason; choose which to import; all are
  saved together or none. POTA (including park-to-park and multi-park lists), SOTA, separate station and
  operator callsigns, UTC dates across midnight, unknown fields kept. Re-importing a file doubles nothing.

**Review before export**
- Every export and ICS 213/214/309 shows exactly which entries it will contain; uncheck any; excluded entries
  are listed with the reason. If an entry is edited or voided before the file is written, the export stops.
  ICS 214 can start from a Traffic Log selection; entries outside the period are blocked, never added.
  Reviewing never changes an entry.

## 2.1.0 — 2026-11-03

Desktop enhancement (spec EMLOG-DESKTOP-ENH-2026-11-03). Logbooks upgrade from 2.0 (schema 3 → 4) with a
verified pre-migration copy; nothing is removed.

**Operator and station**
- Station / assignment per logbook: tactical identifier, position label, licensed station callsign, agency,
  default station position, POTA/SOTA activation.
- Operator bar with **Switch operator…** (handover time and note; unsaved-entry choice). Shift history,
  Traffic Log operator filter and shift-change rows. Entries keep the operator *and* station they were
  logged with.
- HAM station callsign now comes from the station, not the operator's personal callsign.
- Profiles gain licensing country and unit; license class offers common classes.

**Locations and GPS**
- Two locations per entry: *Station position* and *Reported location*, with source, fix time, satellites
  and accuracy.
- **Capture GPS…** (one fix, on request): USB and Bluetooth NMEA receivers on a serial port, gpsd on
  Linux (loopback only), and the computer's location (macOS Location Services, Windows location / built-in
  GNSS, Linux GeoClue) as a labelled fallback. Settings → Location & GPS with **Test GPS**.
- Clear results for no fix, old fix, busy or missing receiver, permission, bad data and timeouts; saving
  never depends on GPS.

**Quick Entry**
- Local suggestions (this logbook only) for callsign, station, channel, frequency, mode, park and summit.
- Save & Add Another keeps session values (band, mode, frequency, channel, station, position, activation)
  and clears contact values.
- POTA/SOTA reference fields with validation.

**Exports**
- ADIF 3.1.8 with presets: General/QRZ, POTA activator, SOTA activator, SOTA chaser; preview of callsigns,
  references, UTC span and every skipped QSO with its reason.
- Station, Station Position, Reported Location (and HAM POTA/SOTA) columns in all report formats.
- Precise coordinates are withheld from shared files unless *Include precise coordinates* is ticked.

**Upgrades**
- Archived 2.0 logbooks are shown from an upgraded temporary copy and upgraded only when unarchived.

## 2.0.0

Operational periods; ICS 213 and ICS 214 on FEMA's official PDFs; ICS 309; channel plan from an ICS 205.

## 1.0.0

Initial release: offline logbooks (General, HAM, Emergency), crash-safe saving, drafts, backups, `.emlog`
packages, reports (PDF, DOCX, XLSX, CSV, TXT, ADIF).
