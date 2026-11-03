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
*Include voided*. The record count is shown before you export. ADIF exports only qualifying QSOs and lists
every skipped entry with the reason. See [EXPORT_FORMATS.md](EXPORT_FORMATS.md).

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
ICS 309 Communications Log is planned for a later phase.

**Upgrading.** The first time this version opens a logbook made by EMCOMM LOG 1.0 it upgrades the file and
keeps an untouched copy in the logbook's hidden `.pre-migration` folder (you are told where).

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
