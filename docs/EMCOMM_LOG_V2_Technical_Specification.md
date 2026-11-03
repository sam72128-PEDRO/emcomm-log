# EMCOMM LOG V2 Technical Specification

Version 0.4 | 3 November 2026 | Builds on V1 1.0 (`EMCOMM_LOG_V1_Technical_Specification.md`)

## 1. Purpose and release boundary

V2 turns the emergency data V1 already stores into the ICS forms operators are asked to hand in, and adds a
channel plan that improves logging itself. Everything in V1 remains in force: offline,
single-user, immediate durable saves, revisions and voids, backups, `.emlog` portability, and the V1
acceptance matrix A01–A15 must keep passing.

V2 adds:

1. **Operational periods** on emergency logbooks (structured start/end), used by every form.
2. **ICS 213 General Message** export, one form per message entry.
3. **ICS 214 Activity Log** export per person/unit and operational period.
4. **ICS 309 Communications Log** export from the Traffic Log.
5. **Channel plan** per operational period, entered by the operator from the ICS 205 they are given, which
   drives channel entry in Quick Entry. The plan is input only; V2 does not export an ICS 205.

**Platform for this build:** V2 is developed, built and tested on **macOS only**. Windows and Linux builds,
CI and installer testing resume once V2 is feature-complete (Phase 5); the code stays cross-platform (no
macOS-only APIs) so that step is a build-and-test pass, not a port.

V2 still excludes: ICS 205 export, ICS 205A Communications List, radio/CAT integration and radio programming exports
(CHIRP etc.), online lookups, cloud sync, accounts, multi-user editing, incident management (staffing,
assignments, multiple teams), electronic signatures, and submitting forms anywhere. No network is required
for any feature.

## 2. Terminology and priority

**Form export:** a filled ICS PDF produced from logbook data. **Official form:** FEMA's published ICS form
PDF. **Operational period (OP):** a named time window with a start and end instant. **Channel plan:** the
list of radio channels in use for an OP. MUST/SHOULD/MAY as in V1. Where this document and V1 conflict on
emergency data, this document controls; on reliability and storage, V1 controls.

## 3. Data additions

All additions are schema migrations with up/down tests (V1 §5 rules). Existing logbooks upgrade in place
on open after the normal pre-migration backup; V1 `.emlog` packages import into V2 and migrate. A V2
package refuses to import into V1 (existing future-schema rule).

### 3.1 Operational periods

Table `operational_periods`: id (UUID), name (e.g. "OP 1", "Day shift 11/03"), `start_utc`, `end_utc`
(RFC 3339; end > start), notes, created/updated, `void` flag. Periods MAY overlap (handover) but the UI
warns. Emergency logbooks manage periods from the Dashboard's incident area (where V1 edits incident
details); other types MAY use them but the forms are emergency-only.

V1's free-text `operational_period` (logbook incident fields and per-entry) is kept and still displayed.
On upgrade nothing is guessed: text that parses unambiguously as a date/time range MAY be offered as a
one-click "Create period from this", never applied silently.

Quick Entry shows the current OP in the header — the active period containing "now" (the latest-starting
one during an overlap) — and stores its `op_id` on new emergency entries; the operator can change it.
Between periods there is no current OP and new entries stay unassigned (the header says so), so nothing
logged after a period ends is attached to it by accident. Entries without `op_id` are placed by event time
when a form is generated.

### 3.2 Message entries (ICS 213 additions)

Add to `EmergencyFields`: `approved_by_name`, `approved_by_position`, `replied_by_name`,
`replied_by_position`, `replied_at_utc`. Shown in the message editor under **Approval** and **Reply**
(reply block only when "Reply requested" is on or a response is entered). Revisions and `.emlog` include
them like every other field.

### 3.3 Resources assigned (ICS 214 block 6)

Table `op_resources`: id, `op_id`, name, ICS position, home agency/unit, sort order. Edited per OP, with
"Copy from previous OP". The logbook's operator profile is not auto-added (it is the 214's own name block).

### 3.4 Channel plan (entered from an ICS 205)

Table `channel_plan`: id, `op_id`, sort order, zone/group, channel number, function, channel name /
talkgroup, assignment, RX frequency (MHz, decimal text), RX N/W, RX tone/NAC, TX frequency, TX N/W, TX
tone/NAC, mode (A/D/M), remarks — the columns of the ICS 205 channel table, so a received 205 can be typed
in row by row. Per OP: `special_instructions` (free text). "Copy plan from previous OP" and "Copy plan from
another logbook" (local only) are provided. The plan is visible on the Dashboard for the current OP.

**Use in logging:** when the entry's OP has a plan, the Channel / Frequency field suggests the plan's
channels first (with function, assignment and mode as hints); choosing one stores "TAC 2 (155.4750 MHz)" —
the channel name and receive frequency. Free text remains allowed (unplanned channels happen). The entry
stores the channel text it was saved with; later plan edits never rewrite saved entries. (HAM logbooks
have no operational periods, so there is no HAM frequency/mode fill.) Plan rows are validated: frequencies
are MHz decimals (a trailing "MHz" is accepted), N/W is N or W, mode is A, D or M; blank rows are dropped.
"Copy from" offers other periods of this logbook and periods with a plan in other local emergency
logbooks, which are opened read-only.

## 4. Form exports

### 4.1 Output contract

- Output is a PDF per export, written atomically and synced like V1 exports.
- **Approach (decision gate, Phase 1):** fill the official FEMA fillable PDFs (public-domain U.S.
  government works, bundled with the app) through their form fields, then flatten nothing — the result
  stays editable in any PDF reader. Phase 1 MUST confirm each bundled form is AcroForm (not XFA-only) and
  record its field names in `docs/ICS_FIELD_MAP.md`. If a form cannot be filled reliably, fall back to
  drawing a faithful layout with the V1 PDF writer; the gate decision is recorded in DECISIONS.md.
- Overflow uses the form's own continuation mechanism (e.g. 214/309 continuation pages); never shrink text
  below legibility, never silently truncate — long text that does not fit continues on an attached
  "Continuation" page referencing the block number.
- Signature fields are always left blank for wet signature. No typed or image signature is inserted.
- Every page carries a small footer outside the form area: "Prepared with EMCOMM LOG <version> from
  logbook <name>, entries #…". Nothing implies the form was approved unless an approver was entered.
- Times are printed in the logbook's display zone with the zone shown, consistent with V1 exports.
- Voided entries are never included. Each export dialog shows the record count and any entries skipped
  with the reason (e.g. "#14 has no recipient — 'To' left blank").

### 4.2 ICS 213 General Message

Source: emergency **message** entries. From Traffic Log selection or entry detail: **Export → ICS 213**.
One form per entry, combined into one PDF in sequence order.

| 213 block | Source |
|---|---|
| Incident name | entry `incident_name`, else logbook incident name |
| To (name, position) | `recipient`, `recipient_position` |
| From (name, position) | `originator`, `originator_position` |
| Subject | entry summary |
| Date / Time | `sent_at_utc` if set, else event time |
| Message | message/details (full text; continuation page if needed) |
| Approved by (name, position) | `approved_by_name`, `approved_by_position`; signature blank |
| Reply | `response` |
| Replied by (name, position, date/time) | `replied_by_*`, `replied_at_utc`; signature blank |

The message number (`message_number`) is printed in the footer, since the 213 has no block for it.

### 4.3 ICS 214 Activity Log

Dialog: choose OP (required), whose log (default: the logbook's operator snapshot; editable name, ICS
position, home agency/unit), entry kinds to include (default: activity; MAY add message/status/note), and
prepared-by (default operator, editable).

| 214 block | Source |
|---|---|
| Incident name | logbook incident name |
| Operational period (from/to) | OP `start_utc`/`end_utc` |
| Name, ICS position, home agency/unit | dialog values |
| Resources assigned | `op_resources` for the OP |
| Activity log (date/time, notable activities) | included entries assigned to the OP, plus unassigned entries with event time inside it (an entry assigned to another OP is never pulled in by time), ordered by event time then sequence; activity text = summary, plus details when "include details" is on; activity start–end shown when both exist |
| Prepared by (name, position, date/time) | dialog; signature blank |

Rows beyond page 1 continue on the form's continuation page(s) with blocks 1–3 repeated.

### 4.4 ICS 309 Communications Log

Source: entries of an OP — assigned to it or unassigned with event time inside it — of the chosen kinds
(default Traffic and Message), in event-time order; voided entries never appear. When started from the
Traffic Log with entries selected, only those entries are logged and any selected entry that cannot appear
is listed with the reason. Dialog: OP, radio net name or position/tactical call, radio operator (name, call
sign — defaults from profile), entry kinds, include details, prepared-by. FEMA publishes no fillable ICS 309,
so the form is drawn by EMCOMM LOG in the standard layout (blocks 1–4 repeated on every page; a 44-line
log per page; blocks 6–8 at the bottom). Every cell wraps onto extra lines instead of being cut; the date is
shown with each time when the OP spans more than one day.

| 309 block | Source |
|---|---|
| Incident name, operational period | as 214 |
| Radio net name / position / tactical call | dialog |
| Radio operator (name, station ID) | dialog / profile callsign |
| Time | event time |
| From (call sign/ID, message #) | `from` field; `message_number` unless direction is sent |
| To (call sign/ID, message #) | `to` field; `message_number` when direction is sent |
| Message | summary, plus details when chosen (wrapped, never cut) |
| 6. Prepared by (name, call sign) | dialog |
| 7. Date & time prepared | time of export |
| 8. Page | page n of N |

## 5. UI

- The emergency Dashboard's incident area (V1's incident details editor) grows into an **Incident** screen
  with tabs: Details (V1 fields), Operational periods, Resources (per OP), Channel plan (per OP).
  Tables are keyboard-editable (Enter to add a row, Tab between cells, Del to remove with confirmation),
  saved immediately with the V1 save contract (Saved / Save Failed, revisions not required for these
  tables but changes are timestamped).
- Traffic Log and entry detail: **Export ▸ ICS 213 / ICS 214 / ICS 309**. Reports & Export screen lists the
  three forms alongside V1 formats.
- Quick Entry: current-OP chip in the header; channel combobox as §3.4.
- All new screens meet V1 accessibility rules (axe clean, labelled cells, focus management).

## 6. Phased implementation and gates

All phases run on macOS: `npm test`, `cargo test --workspace`, a local release build and a hands-on pass
of the new screens. Windows/Linux CI runs only on manual dispatch until Phase 5.

1. **Forms groundwork:** bundle official PDFs, confirm AcroForm vs XFA, write `ICS_FIELD_MAP.md`, prototype
   filling one 213 with a Rust PDF library (pinned, offline). Gate: filled 213 opens correctly in macOS
   Preview, Acrobat Reader, Chrome and Firefox PDF viewers, fields editable; decision recorded.
2. **Operational periods + 213/214:** migrations, OP UI, message approval/reply fields, resources, 213 and
   214 exports. Gate: A16–A19, A22–A24.
3. **309:** Gate: A20.
4. **Channel plan:** migrations, plan editor, Dashboard display, Quick Entry combobox. Gate: A21, A25.
5. **Release:** docs (operator guide "Forms" chapter), then re-enable Windows/Linux CI and installer E2E
   (extended to forms) and build all three platforms; V1 A01–A15 regression on all three.

## 7. Acceptance matrix (additions)

| ID | Test and expected result |
|---|---|
| A16 | Create, edit and void operational periods; end ≤ start rejected; overlaps warned; new emergency entries record the current OP; periods survive restart, backup/restore and `.emlog` round trip. |
| A17 | V1 logbook and V1 `.emlog` open/import in V2 with a pre-migration backup; free-text operational period preserved and never auto-converted. |
| A18 | ICS 213 for a fully populated message fills every mapped block exactly per §4.2; signatures blank; message longer than the box continues on a continuation page with no lost text; multi-select gives one form per message in sequence order. |
| A19 | ICS 214 includes only non-void entries of the chosen kinds that are assigned to the OP or unassigned with event time inside it, ordered by time/sequence; resources block matches the OP; overflow continues on continuation pages with header blocks repeated. |
| A20 | ICS 309 rows match the scope (OP, kinds, Traffic Log selection), with message numbers in the From/To columns per direction; selected entries left out are explained; long cells wrap without loss; counts in the dialog equal entries exported. |
| A21 | Channel plan rows can be entered, reordered, edited and removed by keyboard for each OP; copy from previous OP and from another logbook works; plans survive restart, backup/restore and `.emlog` round trip. |
| A22 | Every form PDF opens in Preview, Acrobat Reader and a browser PDF viewer (macOS during development; Windows/Linux at Phase 5); fields remain editable; Unicode names (accents, ✓) render. |
| A23 | No form contains a signature or approval that was not entered; the "Prepared with EMCOMM LOG" footer is on every page; voided entries never appear. |
| A24 | Approval/reply fields persist through revisions, restart, backup/restore and `.emlog` round trip on another OS. |
| A25 | With a channel plan, the Channel field offers plan channels; choosing one stores the channel name and receive frequency; later plan edits do not change saved entries; free text still allowed. |

## 8. Open questions

- Which FEMA form revisions to bundle (use the current versions on FEMA's ICS forms page at Phase 1, record
  revision dates in `ICS_FIELD_MAP.md`).
- Whether some agencies require their own 213/309 variants (e.g. state or ARES versions); if so, the field
  map design should allow additional bundled templates later without code changes.

## 9. References

- FEMA ICS Forms (ICS 213, 214, 309; 205 for the channel plan columns): https://training.fema.gov/emiweb/is/icsresource/icsforms/
- V1 specification and acceptance matrix A01–A15.
