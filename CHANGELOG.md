# Changelog

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
