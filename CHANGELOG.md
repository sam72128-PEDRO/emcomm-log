# Changelog

## 2.1.0 tablet (Android) — 2026-11-04

All of 2.1.0 on the Android tablet build, on top of the 2.0 tablet port (`docs/ANDROID_TABLET.md`):
- External GPS receivers are not used on the tablet; **Capture GPS** uses the tablet's own location
  (satellite GPS labelled *Device GPS*, otherwise *OS position*), with the location permission asked on
  first use. Manual entry and the station default always work.
- The tablet app now requests the location permission (it still has no network permission).
- ADIF files saved through Android's "Save to…" keep their `.adi` name.

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
