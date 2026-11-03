# Export formats and field mapping

Source of truth: `core/src/export/mod.rs` (`columns()`); the order below is stable and only ever appended to.

## Common column order (all logbook types)

Seq, Status, Event Time (display zone, with offset or `Z`), Event Time UTC (RFC 3339), Entered Zone
(IANA zone + UTC offset as entered), Recorded UTC, Type, Direction, Priority, Handling, Flagged, From, To,
Summary, Message, Channel, Location, Tags, Operator, Operator Callsign, Revision, Voided UTC, Void Reason,
Attachments (display names, `; ` separated).

**HAM** appends: Call, Station Call, Operator Call, QSO Start, QSO End, Frequency MHz, Band, Mode, Submode,
RST Sent, RST Rcvd, My Grid, Their Grid, QSO Notes.

**Emergency** appends: Record Type, Incident, Incident Number, Operational Period, Message Number, Precedence,
Originator, Originator Position, Recipient, Recipient Position, Sender, Receiver, Sent Time, Received Time,
Reply Requested, Action Requested, Response, Resource / Unit, Assignment, Activity Start, Activity End,
Emergency Location, Disposition, Preparer.

## Scope and ordering

- Entire logbook / inclusive date range / current Traffic Log filter (including an explicit selection).
- Date ranges: `[start-of-first-day, start-of-day-after-last)` in the chosen display zone, converted to exact
  UTC instants and shown in the report's scope line.
- Voided entries excluded unless *Include voided*.
- Rows always in event-time order, sequence number as tie-break.
- Every report states logbook name and type, generated time with zone, display zone, scope description and
  record count.

## Per format

| Format | Details |
|---|---|
| CSV | UTF-8 with BOM, RFC 4180 quoting, CRLF between records, LF inside fields. 7 metadata lines, then the header. Cells starting with `= + - @ TAB CR` get one leading `'` (remove it to recover the value). |
| TXT | UTF-8; header block, one section per entry: `#NNNN  time  KIND / direction / priority`, labeled fields, full message indented. |
| XLSX | Sheet *Entries*: frozen header, autofilter, *Event Time UTC* as a typed date-time (UTC), all other values text cells; formula-like text uses quote-prefix style. Sheet *Report*: metadata. Cells over 32,000 characters are truncated with a marker (warning reported). |
| DOCX | Word document, one section per entry (heading kept with its fields), labeled fields, full message paragraphs, attachment names, footer *Page X of Y*. |
| PDF | Landscape Letter, embedded DejaVu Sans, column headings repeated on every page, rows that exceed a page continue on the next, footer with generated time/zone and *Page X of Y*. Text is extractable. |
| ADIF `.adi` | ADIF **3.1.4**. Header: `ADIF_VER`, `CREATED_TIMESTAMP`, `PROGRAMID`, `PROGRAMVERSION`. Only active entries with CALL + QSO start + (BAND or FREQ). |
| `.emlog` | Complete logbook (ZIP: `manifest.json`, `logbook.sqlite`, `attachments/`), independent of report filters. |

## ADIF mapping

| ADIF | Source | Notes |
|---|---|---|
| CALL | their callsign | required |
| QSO_DATE / TIME_ON | QSO start (UTC) | `YYYYMMDD` / `HHMMSS`, required |
| QSO_DATE_OFF / TIME_OFF | QSO end (UTC) | if present |
| BAND | band, else derived from frequency via the ADIF band table | BAND or FREQ required |
| FREQ | frequency in MHz | never fabricated |
| MODE / SUBMODE | mode/submode; USB/LSB→SSB, FT4/JS8/FST4/Q65→MFSK, PSK31/63/125→PSK, DMR/C4FM/FREEDV/M17→DIGITALVOICE | unknown modes omitted with a warning |
| RST_SENT / RST_RCVD | reports | ASCII |
| GRIDSQUARE / MY_GRIDSQUARE | their / my grid | validated Maidenhead |
| STATION_CALLSIGN / OPERATOR | station / operator callsign | |
| COMMENT | QSO notes, newlines → spaces | omitted (warning) if non-ASCII |

Field lengths are the byte counts of the ASCII data. Skipped entries are listed with reasons
(voided, no QSO fields, missing callsign/start/band-and-frequency).

## Validation

`scripts/validate_exports.py` opens the fixtures with independent parsers (Python `csv`, openpyxl,
python-docx, pypdf, adif-io) and checks counts, ordering, escaping, typed dates, long-text integrity,
Unicode, page headings and numbering, and ADIF parsing.
