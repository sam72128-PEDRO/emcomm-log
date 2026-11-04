# EMCOMM LOG operator guide

## 1. First run

The setup wizard appears the first time you open EMCOMM LOG. You can't log until it is finished.

1. **Operator** – your name (required); callsign, license class, grid, organization and position are
   optional. Leave HAM fields blank if you don't operate amateur radio.
2. **Time** – show and enter times in *Local* or *UTC*, 12- or 24-hour clock, theme. Times are always stored
   in UTC; this only changes display and entry.
3. **Storage** – the default folder for new logbooks. Use a local disk or a directly attached USB drive. Network
   shares and cloud-synced folders (iCloud, Dropbox, OneDrive, Google Drive) are refused for live logbooks.
4. **Backups** – a backup folder **outside** your logbook folder (another drive is best), how often to back
   up, and how many copies to keep. You can skip this, but the app will show *Backup not configured*.
5. **Presets** – which starter preset to use for General, HAM and Emergency logbooks.

Everything can be changed later in **Settings** (also reachable when no logbook is open).

## 2. Logbooks (Home)

**Create Logbook** – choose type, name, operator profile, preset and storage folder. Emergency logbooks also
take incident name (required), number, operational period, agency and location. A folder named
`<name>--<id>` is created containing `logbook.sqlite`, `attachments/` and `logbook.json`.

Only one logbook is open at a time. Opening another closes the current one first (with its close backup).

Row actions: **Open**, **Rename**, **Archive/Unarchive** (archived books are read-only; files are never moved),
**Export package (.emlog)**, **Reveal location**, **Remove from list** (forgets it on this computer only; files
stay). If a logbook shows **UNAVAILABLE** (drive unplugged, folder moved), reconnect and press **Retry**, or
**Relink…** to its new location. The app never silently creates a replacement.

## 3. Logging

**Quick Entry** (`Ctrl/Cmd+2`, or `Ctrl/Cmd+N` for a new entry): fill the fields in Tab order and press
**`Ctrl/Cmd+Enter`** to save. *Summary* is required. The entry is written to the logbook's database
immediately; **ENTRY SAVED #n** appears only after the database confirms the commit. The form then clears
for the next entry (untick *Add another after save* to jump to the Traffic Log instead).

- **Date/Time** defaults to now; you may enter a past time. The zone in use is shown under the field and is
  stored with the entry.
- **HAM logbooks**: entry type *QSO* adds callsign (required), start/end, frequency with unit, band, mode,
  RST, grids, station/operator callsigns and notes. Other types are ordinary traffic and are skipped by ADIF.
- **Emergency logbooks**: *Message* adds message number, precedence, originator/recipient, sent/received
  times, reply requested, requested action and response. *Activity* adds resource/unit, assignment, start/end,
  location, disposition and preparer. These are structured log records kept for a future ICS 213/214 mapping;
  they are **not** official forms.
- **Message handling** (Pending, Sent, Awaiting ACK, Acknowledged, …) tracks traffic on the **Messages**
  screen and the dashboard's *Pending Traffic*. Changing the status saves a new revision.
- **Flag for report**, **Tags** (with preset quick tags), **Location**, **Attach file…**.
- **HAM activations**: *My POTA park* / *My SOTA summit* for your own activation, *Their POTA park* /
  *Their SOTA summit* for park-to-park, summit-to-summit or chasing. References are checked and written in
  capitals (`US-0005`, `K-1234`, `W4C/CM-001`). The station's activation (Station dialog) fills *My* fields
  on new QSOs.
- **Suggestions**: callsign, station (From/To), channel, frequency, mode and park/summit fields offer values
  you have already used in *this* logbook, newest first. Nothing is filled in until you pick one; nothing is
  looked up online.
- **Save & Add Another** (*Add another after save*) keeps what belongs to the session — entry type,
  channel, frequency, band, mode/submode, station and operator callsigns, your grid, your park/summit and the
  station position — and starts the rest fresh, including their callsign, reports, their grid/park/summit,
  summary, message text, notes and the reported location.
- **Templates**: click a quick template, or use the *Templates* tab. *Save as Template* stores the current
  fields (never times) in this logbook. **Favorites**: star entries in the Traffic Log; the *Favorites* tab
  can start a new entry from one.

### Drafts
While you type, the unfinished entry is saved as a **draft** about two seconds after you stop
(**DRAFT SAVED** in the top bar). A draft is *not* a logbook entry. If the app or computer stops
unexpectedly, the next time you open the logbook you are offered **Restore**, **Keep for Later** or
**Discard**. Restoring puts the text back in the editor; nothing is saved until you press Save. Drafts kept
for later are under **Settings → Storage & Recovery**.

### If saving fails
**SAVE FAILED** appears with the reason (drive missing, disk full, permission, database busy). Your text stays
in the form and in a local draft, and nothing is added to the logbook. If the logbook location itself is the
problem, further final saves are blocked: reconnect the drive or fix the folder, then **Recheck storage** and
save again. If the drive is lost, close the logbook and **Restore Backup** (section 5); re-enter or restore
your drafts there.

### Operator, station and shifts (2.1)

The **operator** is the person at the radio (a profile in *Settings → Operator Profiles*: name, callsign,
license class and country, organization, unit). The **station** is the desk, position or portable setup —
**Logbook → Station / assignment** in the operator bar: tactical identifier (e.g. `MARC-5 COMMS`), position
label, licensed station callsign (e.g. a club call), agency, default station position, and for HAM the park
or summit being activated. A tactical identifier is not a licensed callsign. In HAM logbooks the QSO's
*Station callsign* comes from the station, and *Operator* from the person on duty — never one from the other.

The operator bar (Dashboard and Quick Entry) shows **ON DUTY: name** and the station. **Switch operator…**
records a handover at one instant (now, or an earlier time in the current shift) with an optional note.
It does not end the operational period, change ICS forms, or change entries already saved — each entry keeps
the operator and station it was logged with. If an entry is half-typed, you choose: save it now under the
outgoing operator, or leave it open for the incoming operator to finish (it is then recorded under them).
The Traffic Log can filter by operator and shows each shift change as a divider row.

### Locations and privacy (2.1)

Each entry can carry two separate locations, in the collapsible *Location* section:

- **Station position** — where the logging station is. *Capture GPS…* takes one fix (see
  [GPS_SETUP.md](GPS_SETUP.md)), *Use default* copies the station's default position, or type it.
- **Reported location** — a place mentioned in the traffic (a road closure, a shelter). Never filled from GPS.

Each keeps its source (USB/Bluetooth receiver, gpsd, internal GNSS, OS position, logbook default, manual,
reported), fix time, satellites and reported accuracy. Typing over a captured position makes it *manual*.
Coordinates are WGS84 decimal degrees; both or neither.

Exports leave out precise latitude/longitude by default and shorten grids to 6 characters; place names
stay. Tick **Include precise coordinates** on the Reports screen to include them. `.emlog` packages and
backups are full copies and always contain them.

### Attachments (2.2)

The **Attachments** box (full Quick Entry and Edit) takes files three ways: **Attach files…** (choose several),
**drag and drop** onto the box from Finder / File Explorer / your file manager, or **paste** a screenshot: click
the small *paste an image* field in the box and press `Ctrl/Cmd+V` (pasting anywhere else still pastes text as
usual). Each file is copied into the logbook straight away, so the original may be on a USB stick or deleted
later. Large files (25 MB and up) ask first; a copy in progress shows a progress bar and **Cancel**.

Add a **caption** to any attachment. A new entry's attachments belong to it only when you save it; until then
they are kept with the recovery draft. On a saved entry, **Remove** marks an attachment for removal and Save
Changes records it as a change; the file itself is kept for history and backups.

Entry Detail shows small previews of photos (made without the photo's hidden EXIF data, such as the GPS
position), the caption, size and where it came from, with **Open** and **Save As…**. EMCOMM LOG never opens a
file by itself, and never opens programs or scripts at all (use Save As). Reports list attachment names and
captions only; the files themselves are never put in a report. A `.emlog` package or backup contains the
original files, including any location data inside photos.

### Important and follow-up (2.2)

Next to *Flag for report* are **Important** and **Follow-up needed**. A follow-up can have a due time and a
short note, and is *Open* until you mark it *Resolved*. In Entry Detail: **Mark Important**, **Follow-up
needed**, **Resolve follow-up**, **Reopen follow-up**; each is saved at once as a new revision. These are
separate from Flag for report (which ICS reports use) and from the message status (Pending, Acknowledged…):
resolving a follow-up is not an acknowledgement. The Dashboard's **Open Follow-ups** card counts them (and how
many are overdue) and opens the Traffic Log filtered to them. No reminders or notifications are sent.

### Related entries (2.2)

In Entry Detail, **Related entries** lists links to other entries in this logbook: *Related to*, *Response to*,
*Follow-up to*, *Relay of*, *Corrects* (a new message correcting an earlier one; to fix a typo, Edit the entry
instead). **Link existing entry…** searches by number (`#12` or `12`) or text; ↑ ↓ and Enter work. Click a
number to open that entry. A link changes neither entry and adds nothing to the log; removing a link asks first
and is recorded. A voided entry keeps its links, shown as VOIDED. The Traffic Log shows how many links each
entry has.

### Logbook directory (2.2)

**Settings → Logbook Directory** (while a logbook is open) holds the stations you work with in *this*
logbook: a name, type (amateur callsign, tactical identifier, station, organization, other), identifier,
agency, preferred channel/frequency/mode, also-known-as names, tags and notes. **Save to Directory…** in Entry
Detail starts one from an entry. While typing From/To or a callsign, directory items are suggested first
(marked *Directory*); when the text matches one, **Use these** fills its channel, frequency and mode, only if you
click it. Editing or removing a directory item never changes entries already saved. Each logbook has its own
directory, and it travels in `.emlog` packages.

## 4. Traffic Log, corrections, void

`Ctrl/Cmd+3` (search: `Ctrl/Cmd+F`). Filter by text, type, station/callsign, period, status, and *More filters*
(direction, priority, handling, tags, favorites, flagged, sort by event or recorded time). Arrow keys move
between rows, **Enter** opens details, **Space** selects rows for **Export Selection**. **Print** makes a PDF
of the current filter and opens it.

Entry details show every field, attachments (click to open) and the **revision history** with exactly which
fields changed. **Edit** → *Save Changes* keeps the same entry number and records the before/after and an
optional reason. **Void** (with optional reason) hides an entry from the default list and reports; it is not
deleted and its number is never reused. Show voided entries with the *Status* filter; **Restore** makes it
active again. There is no hard delete.

## 5. Backups and restore

With a backup folder configured, a verified backup is made every interval while a logbook is open (skipped if
nothing changed), when you close the logbook, and when you choose **Back Up Now** (`Ctrl/Cmd+Shift+B`).
Each backup is a dated folder under `<backup folder>/<logbook>--<id>/` with a manifest of SHA-256 hashes; it
only counts as successful after every file and the database are verified. A failed backup never replaces an
older one. Old backups are pruned only after a newer one succeeds (default: newest 8, plus one per day for 7
days and one per week for 4 weeks).

**Restore Backup** (Home): pick the backup folder, select a backup, review the verified summary, choose a name
and destination. It is restored as a **new, separate logbook**; existing logbooks are never overwritten.

## 6. Moving a logbook to another computer (.emlog)

**Reports → Export .emlog package** (or the package button on Home) writes one file containing the complete
logbook: all entries including voided ones, revision history, templates, favorites, logbook settings and
attachments. Operator profiles, app settings and drafts are not included.

On the other computer: **Home → Import Package…**, choose the file, review, pick a folder. Import validates the
package (format version, file list, hashes, database integrity, unsafe paths) and always creates a new local
logbook. Importing a package that came from a logbook already on that computer is reported but never merges
or overwrites.

## 7. Reports

`Ctrl/Cmd+E`. Choose **PDF, DOCX, XLSX, CSV, TXT**, or **ADIF** (HAM logbooks). Scope: entire logbook, an
inclusive date range, or the current Traffic Log filter/selection. Voided entries are excluded unless you tick
*Include voided*. The record count is shown before you export. See [EXPORT_FORMATS.md](EXPORT_FORMATS.md).

**ADIF presets** (HAM): one standard ADIF 3.1.8 file; the preset chooses which QSOs qualify.

| Preset | Includes | Adds |
|---|---|---|
| General / QRZ | every QSO with a callsign, UTC start and band or frequency | — (for QRZ, LoTW via TQSL, Club Log…) |
| POTA activator | QSOs with *My POTA park*, a station or operator callsign, and a mode | `MY_SIG=POTA`, `MY_SIG_INFO`; `SIG`/`SIG_INFO` for park-to-park |
| SOTA activator | QSOs with *My SOTA summit*, a callsign and a mode | — |
| SOTA chaser | QSOs with *Their SOTA summit* and a mode | — |

Before exporting you see the station callsign(s), operators, references and UTC span, and **every entry that
will be left out with its reason**; tick *Export without these* to go ahead. EMCOMM LOG never uploads: import
the file on the service's website.

### Review before export (2.2)

**Review and export…** lists every entry the export would contain, with a checkbox for each. Uncheck what you
do not want, or use *Select all eligible* / *Deselect all*; entries that cannot be included (voided, not a QSO
for ADIF, missing data for the preset) are listed with the reason. Press Enter on a row to open that entry.
Then choose where to save. If someone edits or voids one of the reviewed entries before the file is written,
the export stops and names the entry; review it again. Reviewing never changes an entry. Above 10,000 entries
the review shows the counts only and exports the whole selection.

The ICS 213, 214 and 309 dialogs show the same review. **ICS 214** can also start from a Traffic Log selection
(**ICS 214…**): entries outside the chosen operational period are shown as blocked and are never put on the
form.

### Import QSOs from ADIF (HAM logbooks, 2.2)

**Reports → Import ADIF…**, then choose an `.adi` file from QRZ, LoTW, POTA, SOTA or another program. Before
anything is saved you see every QSO: **New**, **Duplicate?** (same callsign, UTC minute, band, mode, station
callsign and park/summit as a QSO already in the log, earlier in the file, or imported before) and **Cannot
import** (with the reason, e.g. a bad date). New QSOs are ticked; tick a duplicate to import it anyway. If the
file has no station or operator callsign for a QSO, you can give one; values in the file always win, and your
profile and this logbook's station are not changed. Each QSO becomes a new entry with the next number and the
QSO's UTC start as its time; you are recorded as who imported it. All chosen QSOs are saved together, or none
are if anything fails. A record listing several parks becomes one QSO per park. Fields EMCOMM LOG does not use
are kept with each QSO. The result shows a summary you can copy.

### Operational periods and ICS forms (Emergency logbooks)

**Operational periods.** On the Dashboard, *Operational Periods* → **New period** (name, start, end). While a
period is running, Quick Entry shows **PERIOD: name** and new entries are tagged with it; you can change an
entry's period in the *Incident* section of the full editor. Between periods the header shows **NO CURRENT
PERIOD** and new entries are placed by their time. Periods may overlap for a handover (you get a warning).
Periods are voided, never deleted; entries keep their period. **Resources** lists the people assigned to a
period (ICS 214 block 6), with *Copy from* another period.

**ICS 213 General Message.** Select one or more *Message* entries in the Traffic Log and choose **ICS 213**,
or open a message and choose **ICS 213…**. Each message becomes one filled ICS 213 on FEMA's official form:
To/From, subject, date and time (sent time if set), message, approved by, reply, and replied by — fill the
*Approved by* and *Replied by* fields in the message's details. Text too long for a block continues on an
attached continuation page that names the block. Non-message and voided entries are listed as not included.

**ICS 214 Activity Log.** Dashboard → period → **214**, or *Reports & Export* → **ICS 214 Activity Log…**.
Choose the period, whose log it is, which entry types to list (Activity by default; Message, Traffic, Status
and Note can be added) and who prepared it. The log lists entries assigned to that period plus unassigned
entries whose time falls inside it, in time order: 24 rows on the first page, 36 on each continuation page.

All forms: signature lines are left blank for signing after printing, the fields stay editable in any PDF
reader, voided entries never appear, and a small footer says the form was prepared with EMCOMM LOG. The
ICS 309 is drawn by EMCOMM LOG in the standard layout, because FEMA does not publish a fillable one.

**ICS 309 Communications Log.** Dashboard → period → **309**, *Reports & Export* → **ICS 309 Communications
Log…**, or the Traffic Log's **ICS 309…** button (with entries selected, only those are logged). Enter the net
name and radio operator; choose entry types (Traffic and Message by default). Each entry is one log line:
time, FROM call sign/ID, TO call sign/ID, and the message summary. A received message's number goes under
FROM, a sent message's under TO. Long text wraps onto extra lines; 44 lines per page, with the header
repeated.

**Channel plan (from an ICS 205).** Dashboard → period → **Channels** (or **Enter ICS 205…** in the
*Channel Plan* panel, which shows the current period's plan). Type one row per channel — zone/group,
channel number, function, channel name or talkgroup, assignment, RX/TX frequency in MHz, N/W, tone/NAC,
mode (A/D/M) and remarks — plus the special instructions. Enter adds a row. *Copy from* fills the editor
from another period, or from a period in another emergency logbook on this computer (that logbook is only
read). While a plan exists, every entry's *Channel / Frequency* field suggests its channels first; picking
one writes e.g. "TAC 2 (155.4750 MHz)". You can still type anything. Changing the plan later never changes
entries already saved. EMCOMM LOG does not print an ICS 205.

**Upgrading.** The first time this version opens a logbook made by EMCOMM LOG 1.0 it upgrades the file and
keeps an untouched copy in the logbook's hidden `.pre-migration` folder (you are told where). 2.1 does the
same for 2.0 logbooks (schema 4: station, shifts, locations, park/summit references). An **archived** logbook
is not changed when you view it — you see an upgraded temporary copy — and is upgraded only when you
unarchive it.

## 8. Where your data lives

| Data | Location |
|---|---|
| Logbook (entries, revisions, templates, favorites, logbook settings) | `<your folder>/<name>--<id>/logbook.sqlite` |
| Attachments | `<your folder>/<name>--<id>/attachments/<sha256>.<ext>` |
| Settings, operator profiles, presets, logbook list, backup history | app-data folder `app.sqlite` |
| Drafts | app-data folder `drafts/<logbook id>/` |
| Backups | your backup folder |

App-data folder: macOS `~/Library/Application Support/org.emcommlog.desktop`, Windows
`%APPDATA%\org.emcommlog.desktop`, Linux `~/.local/share/org.emcommlog.desktop`. **Help → Where Is My Data?**
shows the exact paths and can reveal them. Never copy `logbook.sqlite` alone while the logbook is open —
use a backup or `.emlog` package.

## 9. Keyboard

| Keys | Action |
|---|---|
| Ctrl/Cmd+N | New entry |
| Ctrl/Cmd+Enter | Save Entry / Save Changes |
| Ctrl/Cmd+F | Find in Traffic Log |
| Ctrl/Cmd+E | Export |
| Ctrl/Cmd+1…5 | Dashboard, Quick Entry, Traffic Log, Messages, Reports |
| Ctrl/Cmd+, | Settings |
| Ctrl/Cmd+Shift+O / B | Logbooks / Back Up Now |
| Ctrl/Cmd+/ | Shortcut list |
| Esc | Close dialog / cancel |

While a dialog is open, shortcuts other than Save are disabled and a screen-reader message explains why.
