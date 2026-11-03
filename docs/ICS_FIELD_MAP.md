# ICS form field map

Official FEMA NIMS ICS forms bundled in `core/assets/ics/`, downloaded 2026-11-03 from
https://training.fema.gov/emiweb/is/icsresource/icsforms/ (public-domain U.S. government works).

| File | Form | SHA-256 |
|---|---|---|
| `ICS-213-v3.pdf` | ICS Form 213, General Message (v3) | `e745a3a7b78bea3295e33b1efbc291011a9b78bdd1c937115c8c5301e7b9454f` |
| `ICS-214-v3.1.pdf` | ICS Form 214, Activity Log (v3.1) | `fe7d710eb7c188c9b114b6dad08ed4ee72bf0883fc5f7b59f2f519e4fec3b87b` |

Both are **AcroForm** (no XFA), unencrypted, US Letter (612×792 pt). Default appearance `/Helv 0 Tf`
(Helvetica, auto size) and no `NeedAppearances`, so EMCOMM LOG writes its own appearance streams with the
embedded DejaVu Sans (Unicode) and sets each filled field's `/DA` to that font. Instruction pages (213
page 2, 214 page 3) are dropped from exports. Signature fields (`/Sig`) are never filled.

**ICS 309** is not part of FEMA's NIMS v3 form set (not on the FEMA page; NWCG's form list could not be
reached from this machine). Per the V2 spec fallback, the 309 is drawn by EMCOMM LOG's own PDF writer with
the standard 309 layout — see DECISIONS.md.

## ICS 213 General Message (v3) — page 1

| Field name | Type | Rect (pt) | Source (V2 spec §4.2) |
|---|---|---|---|
| `1 Incident Name Optional` | text | 404×15 | entry incident name, else logbook incident name |
| `2 To Name and Position` | text, multiline | 412×28 | `recipient`, `recipient_position` → "Name, Position" |
| `3 From Name and Position` | text, multiline | 399×27 | `originator`, `originator_position` |
| `4 Subject` | text, multiline | 338×28 | entry summary |
| `5 Date` | text | 68×18 | sent time, else event time (date, display zone) |
| `6 Time` | text | 68×18 | same instant, time + zone |
| `7 Message` | text, multiline | 533×246 | message/details; overflow → continuation page |
| `8 Approved by Name` | text | 99×13 | `approved_by_name` |
| `Signature_19` | signature | — | never filled |
| `PositionTitle_13` | text | 96×13 | `approved_by_position` |
| `9 Reply` | text, multiline | 533×239 | `response`; overflow → continuation page |
| `10 Replied by Name` | text | 103×13 | `replied_by_name` |
| `PositionTitle_14` | text | 102×13 | `replied_by_position` |
| `Signature_20` | signature | — | never filled |
| `DateTime_14` | text | 262×13 | `replied_at_utc` (display zone) |

No message-number block: the number goes in the EMCOMM LOG footer.

## ICS 214 Activity Log (v3.1)

Page 1 (first page) and page 2 (continuation). Fields with the same name on both pages are one field with
two widgets (same value): `Date From`, `Date To`, `Time From`, `Time To`.

| Field name(s) | Page | Source (V2 spec §4.3) |
|---|---|---|
| `1 Incident Name_19` / `1 Incident Name_20` | 1 / 2 | logbook incident name |
| `Date From`, `Time From`, `Date To`, `Time To` | 1 + 2 | OP start / end (display zone) |
| `3 Name`, `4 ICS Position`, `5 Home Agency and Unit` | 1 | dialog (defaults from operator profile) |
| `NameRow{1..6}_3`, `NameRow{7,8}`; `ICS PositionRow{1..8}`; `Home Agency and UnitRow{1..8}` | 1 | resources assigned, 8 rows |
| `DateTimeRow{1..24}`, `Notable ActivitiesRow{1..24}` | 1 | activity rows 1–24 |
| `DateTimeRow{1..24}_2`, `Notable ActivitiesRow{1..24}_2`, then `DateTimeRow{25..36}`, `Notable ActivitiesRow{25..36}` | 2 | continuation rows (36 per page) |
| `8 Prepared by Name` / `8 Prepared by Name_2` | 1 / 2 | prepared-by name |
| `PositionTitle_15` / `PositionTitle_16` | 1 / 2 | prepared-by position |
| `Signature_21` / `Signature_22` | 1 / 2 | never filled |
| `DateTime_15` / `DateTime_16` | 1 / 2 | prepared date/time |

Each activity row is one line (≈444 pt wide, 15 pt tall); text that does not fit one row continues on the
next row with the Date/Time cell blank. More than 60 rows: page 2 is cloned with its fields renamed
(`…#3`, `…#4`) so each continuation page holds its own values. More than 8 resources continue on the
EMCOMM LOG continuation page.
