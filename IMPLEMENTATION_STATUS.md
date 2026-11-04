# Implementation status (acceptance matrix A01–A15)

Environments: local macOS 14.6.1 (Apple Silicon), Rust 1.99.1, Node 26 — the macOS build is made and
tested here; plus GitHub Actions native runners **windows-latest, ubuntu-22.04** (private repo
`sam72128-PEDRO/emcomm-log`). CI (format, lint, typecheck, frontend tests, clippy `-D warnings`, all Rust
unit/integration tests, independent export validation, installer builds, Linux⇄Windows `.emlog` round
trips) is green on both, and earlier runs ([41587742360](https://github.com/sam72128-PEDRO/emcomm-log/actions/runs/41587742360))
were green on macOS runners too before macOS moved to local builds (GitHub's macOS runners were
unavailable for capacity).

**Final sweep (2026-11-03).** Installed-app WebDriver E2E
([run 41664446060](https://github.com/sam72128-PEDRO/emcomm-log/actions/runs/41664446060)): **50/50 checks
pass on Windows (`.msi`) and 50/50 on Linux (`.deb`)**, including axe-core on 10 screens with no
serious/critical violations, native menu automation, and the OS accessibility tree (UIA / AT-SPI). Installer
smoke passes for `.msi`, NSIS `.exe` (firewalled), `.deb`, `.AppImage`, `.rpm` (no network). macOS: locally
built `.dmg` passes `scripts/mac_smoke.sh` (IP networking denied, data store created, wizard rendered) and
imports the Linux- and Windows-produced `.emlog` packages from CI.

CI found and fixed three Windows-only defects: migration checksums broke when Git converted SQL files to CRLF
(logbooks would not move between Windows and Unix), export fsync used a read-only handle (every Windows export
failed), and a `windows-sys` constant import.

Status key: **passing** (evidence below) · **partial** (passes where it could be run; named gaps) · **blocked**
(needs an environment this run did not have).

| ID | Status | Evidence |
|---|---|---|
| A01 First launch blocks use until settings saved; settings editable without a book | **passing (manual on macOS; settings tests on all 3 OSs)** | Release app: wizard shown on empty data dir; empty name blocked with inline error; settings persisted to `app.sqlite` and Home + Settings reachable with no book. `appdb::tests::settings_round_trip_and_validation` (validation, persistence, schema version). Failed settings write keeps the wizard open (error path in `Wizard.finish`). |
| A02 Create General/HAM/Emergency from profile/preset; independent after preset edits | **passing** | `storage::a02_each_type_from_preset_keeps_independent_settings`; Emergency book created in the release app. |
| A03 Save Entry / Save Changes commit at live path; restart retains exact entry & revision | **passing** | `storage::a03_save_entry_and_changes_persist_at_live_path_without_extra_save` (independent connection reads the live file), `storage::a03_committed_entry_survives_process_kill` (child process commits then `abort()`s; reopen finds exact entry/revision, stale-lock integrity check). Release app: entry saved, app quit, `sqlite3` on the live file showed entry #1 rev 1, WAL checkpointed; relaunch showed it. Pragmas verified on open (`wal`, `synchronous=2`, `foreign_keys=1`, macOS `fullfsync`). |
| A04 Concurrent saves unique/monotone; void/restore never reuse; retry never duplicates | **passing** | `storage::a04_concurrent_saves_get_unique_monotone_numbers` (4 independent connections × 25, then 4 threads × 10 via shared handle → 1..140 exactly), `a04_retry_token_never_duplicates_and_void_never_reuses_numbers` (duplicate token, rolled-back attempt consumes nothing, stale base revision rejected). Frontend test: double-click submits once; identical retry reuses the request id. |
| A05 Missing drive / disk full / lock: Save Failed, editor kept, no false entry, draft survives | **passing (all 3 OSs; UI on macOS)** | Real failures: `a05_real_sqlite_full_rolls_back` (SQLITE_FULL), `a05_missing_drive_blocks_final_saves_and_draft_survives` (folder moved), `a05_database_busy_reports_busy`, `a05_permission_denied_on_attachment_write` (chmod), `a05_second_process_cannot_open_same_logbook`. Injected: disk-full/permission/IO/busy before commit. Frontend test: failure keeps content, writes draft, blocks saves until recheck. Release app: moved live folder mid-session → SAVE FAILED + DRAFT SAVED + text kept; moved back → Recheck → saved as #3. |
| A06 Kill mid-draft; recovery offer with accurate source/time; never silently finalizes | **passing** | `storage::a06_draft_recovery_after_kill_does_not_finalize` (child writes draft then aborts). Frontend test: draft written ~2 s after typing, not before. Release app: typed entry, `kill -9`, reopen → *Recover unfinished entries* with time; Restore reloaded exact fields incl. original event time; no entry created until Save. |
| A07 Edit/void/restore timeline; default hides voided; filter reveals | **passing** | `storage::a07_edit_void_restore_timeline_and_filters` (actions/revisions, immutability triggers, unchanged edit adds nothing). Release app: edit showed field diff in timeline; void with reason hid the row from default list. |
| A08 Auto/manual/close backups independent; partial not marked successful; retention | **passing** | `protection::a08_backups_are_independent_verified_and_retained` (three kinds, verification, independence, injected failure leaves no temp and keeps prior backups, tampered backup fails verification, retention prune), `a08_backup_fails_cleanly_when_destination_missing`, unit `retention_keeps_recent_daily_weekly`. Release app: close backup and 60-s auto backup recorded `Verified`. |
| A09 Restore creates distinct book; failed validation leaves books untouched | **passing (core)** | `protection::a09_*` (new identity when UUID known, entries/attachments/hashes/sequence preserved, numbering continues, corrupted backup → no new folder, name collision → no leftovers), `restore_keeps_uuid_when_not_known_locally`. The Restore dialog UI was built but not clicked through in the release app. |
| A10 `.emlog` on another OS preserves data; malformed/oversized/traversal/duplicate safe | **passing** | CI cross-OS round trips Linux→Windows and Windows→Linux pass; the Linux and Windows packages also import on macOS (local `crossos` run); earlier CI runs also passed Windows→macOS and macOS→Linux (`crossos.rs`: Unicode names, attachments+hashes, revisions, void, favorites, templates, logbook settings). | Same-OS: `protection::a10_a13_package_round_trip_preserves_everything` (all 3 types; entries, revisions, templates, favorites, settings, attachments, Unicode names; duplicate import reported, never overwrites), `crossos::same_os_round_trip_matches_crossos_expectations`. Malformed: traversal, absolute, backslash, unexpected file, WAL sidecar, missing manifest, future format/schema, wrong format, hash mismatch, malformed manifest, non-SQLite DB, symlink, duplicate names, truncated archive — all rejected with no leftovers. Oversize limits are enforced in code but no >4 GB fixture was built. |
| A11 DOCX/XLSX/PDF/CSV/TXT match scope, order, timestamps, void, long text; no Office | **passing** | Fixtures generated and validated by independent parsers on Windows, Linux and macOS in CI. | `exports::a11_*` and `scripts/validate_exports.py`: 43/43 independent checks pass (csv, openpyxl, python-docx, pypdf). PDF rendered with macOS Quick Look. Release app DOCX export read back with python-docx. 100k-entry streaming exports in `scale.rs`. Not opened in desktop viewers (Word/LibreOffice/Acrobat) on Windows/Linux. |
| A12 ADIF only eligible QSOs, valid lengths/dates, skipped report | **passing** | `exports::a12_adif_contains_only_eligible_qsos_with_valid_fields` (independent ADI reader checks byte lengths), adif-io parses the fixture (4 QSOs, UTC times). |
| A13 Emergency message/activity fields persist & round trip; not presented as official | **passing** | `protection::a10_a13_*` (message & activity fields, incident snapshot), `exports::emergency_exports_do_not_claim_official_forms`; UI labels say "not an official form". |
| A14 Menus/shortcuts on all OSs; focus, Tab, Esc, screen-reader status | **passing (automated on Windows and Linux, by hand on macOS); no human screen-reader session** | Installed-app E2E (`scripts/e2e.py`, 50/50 on Windows and Linux): Ctrl+1–5/F/,/Enter shortcuts, Tab order, focus into dialogs and restored after Esc, focus to invalid field, shortcuts blocked under dialogs with announcement, save and validation announced in live regions, native View menu driven by OS automation (pywinauto / xdotool), and the OS accessibility tree (UIA for Narrator/NVDA, AT-SPI for Orca) exposes labelled fields, buttons and combos. axe-core: no serious/critical violations on 10 screens. | macOS release app: native menus with shortcuts, Cmd+1–5/E/Enter, F1, Tab order, Esc and dialog focus verified by hand. Not done: a session with a real screen reader (VoiceOver/NVDA/Orca) by a person. |
| A15 Native installers launch offline on clean machine; no placeholders/demo data | **passing** | `installer-smoke.yml` on fresh GitHub VMs with the CI installers ([run 41664446060](https://github.com/sam72128-PEDRO/emcomm-log/actions/runs/41664446060)): **Linux** `.deb` (apt), `.AppImage`, and `.rpm` (clean Fedora container) launched with no network, stayed running, created their data store, wizard rendered; **Windows** `.msi` and NSIS `.exe` installed silently and run with the app's outbound traffic blocked by firewall rule, same checks. **macOS** (built locally): `scripts/mac_smoke.sh` mounts the `.dmg`, runs the app with all IP networking denied (Seatbelt), checks it stays up, creates its data store and shows the wizard. | Smoke screenshots exposed and verified a fix for the window running off small (1024x768) screens. Not covered: Gatekeeper/SmartScreen first-launch prompts for unsigned builds. No demo data is shipped. |

## 2.1.0 — desktop GPS, Quick Entry, HAM presets, operator/station (spec EMLOG-DESKTOP-ENH-2026-11-03)

**Where it was verified (2026-11-03).**
- **macOS 14.6 (this Mac):** every core and frontend test passes, clippy is clean, and svelte-check is clean.
  The 2.1.0 `.dmg` passes `scripts/mac_smoke.sh` (runs with networking denied, creates its data store, and
  `Info.plist` has the location usage strings).
- **Linux (Debian container, Rust 1.99.1):** all core tests pass, run as a non-root user, including the
  pseudo-terminal serial test. Clippy with `-D warnings` is clean on the core (GeoClue code included) and on
  the full Tauri app crate.
- **Windows:** clippy with `-D warnings` is clean on the core for `x86_64-pc-windows-gnu` (Geolocator code
  included). Nothing ran on Windows itself.
- **GitHub Actions:** not run for 2.1. By the product owner's decision, Windows/Linux builds and CI wait
  until the macOS version is finished, so the Windows/Linux installers and installed-app E2E are **not yet
  run** for 2.1.
- **Hands-on (macOS release build, scratch data, 2026-11-03):** clicked through:
  - first-run setup and a second operator profile;
  - the Station dialog: tactical ID, station callsign normalised, default park, typed default position;
  - a HAM QSO, with the station callsign from the station, the operator callsign from the person on duty,
    and My POTA from the station;
  - Switch Operator both ways with an unsaved QSO open (save under the outgoing operator; leave it for the
    incoming operator, whose callsign it then takes);
  - the Traffic Log shift-change row and operator filter;
  - Use default and a reported location on one entry, and Entry Detail;
  - local callsign suggestions and Save & Add Another carry-over;
  - draft recovery after the app was killed;
  - the ADIF preview and presets (SOTA activator 0 eligible with reasons and Export disabled; POTA
    activator exported, with MY_SIG/SIG, UTC date across local midnight, and MY_LAT/MY_LON only with the
    opt-in);
  - a CSV with coordinates withheld.

  The hands-on pass found and fixed 7 defects:
  1. Save & Add Another carried the previous operator's callsign across a handover.
  2. Esc discarded the Station dialog's unsaved changes without asking.
  3. "Timed out" was shown while macOS was still waiting for the user to answer its permission prompt.
  4. No grid was computed from typed coordinates.
  5. The Switch Operator dialog showed "No shift yet" for the operator shown in the header.
  6. "accuracy unknown" appeared on typed positions.
  7. POTA/SOTA references were missing from Entry Detail.

| ID | Status | Evidence |
|---|---|---|
| DESK-GPS-01/02 USB NMEA receiver, each OS | **pending (hardware)** | No receiver was available. The serial path is covered by a pty test on Linux and by recorded-NMEA parser tests. |
| DESK-GPS-03 Bluetooth serial | **pending (hardware)** | Same code path as USB once the OS exposes a port; documented in `docs/GPS_SETUP.md`. |
| DESK-GPS-04 gpsd over loopback | **passing (simulated gpsd)**, real gpsd **pending** | `gps::tests::gpsd_end_to_end_over_loopback`; non-loopback addresses are refused; absent gpsd → `no_device`. |
| DESK-GPS-05 OS location, labelled | **partial** | macOS: Capture GPS asks Core Location, and macOS shows its permission prompt (confirmed in the `locationd` log). Without an answer the dialog now explains that and offers Retry. A real macOS fix has not been seen yet (the prompt was not answered during the test). Linux/Windows: compile only. |
| DESK-GPS-06 Bad data never gives a fake fix | **passing** | NMEA tests: bad checksum, RMC `V`, GGA quality 0, stale fix, never-ending garbage, missing port, cancellation. |
| DESK-GPS-07 Internal GNSS when present | **partial** | Windows: a satellite-sourced Geolocator position maps to *internal GNSS*; a built-in serial GNSS uses the serial path. A missing device is a normal outcome. Not run on hardware. |
| DESK-GPS-08 Manual and default location with no GPS/network | **passing (core, all 3 OS builds compile)** | `identity::desk_loc_01_*`, `validate_location` tests. |
| DESK-OP-01/02/03 | **passing** | `identity.rs`: the station survives a switch, periods are untouched, and profile edits do not rewrite entries. |
| DESK-OP-04 Unsaved entry at handover | **passing (by hand on macOS)** | Both choices were clicked through in the release build: *save it now* saved the QSO under Alice before Bob's shift; *leave it open* kept the QSO and its operator callsign changed to the incoming operator. `followIdentity` and `carrySession` are unit-tested. |
| DESK-LOC-01/02 | **passing** | Two independent locations. Every GPS failure is a typed outcome and saving never depends on it. |
| DESK-QE-01 | **passing** | `recents` test (local, newest first, voided excluded); vitest Save & Add Another retention and no leakage; HAM station callsign comes from the station. |
| DESK-QE-02 | **passing** | V1 save contract unchanged (A03–A06 suites). |
| DESK-HAM-01 | **passing** | `exports::desk_ham_presets_*` and `scripts/validate_exports.py` with adif-io: 52/52, covering the POTA preset, P2P SIG, MY_LAT/MY_LON, UTC midnight and Unicode. |
| DESK-HAM-02 | **passing** | Invalid POTA/SOTA refs are rejected on save; every QSO a preset leaves out is listed with its reason and needs acknowledgement. |
| DESK-EXP-01 | **passing (core)** | All V1/V2 export, form and package suites pass on macOS and Linux. |
| DESK-MIG-01 | **passing** | `identity::desk_mig_01_*`: 2.0 logbook → schema 4 with verified copy; `.emlog` round trip. An archived logbook is viewed via a temporary copy. The Android branch has not yet been rebased onto 2.1. |
| DESK-PRIV-01 | **passing** | `exports::desk_priv_*`: coordinates withheld and grids cut to 6 by default, opt-in per export. The `.emlog` section warns that it holds precise positions. |

Known limitations in 2.1: there is no SOTA CSV export (only ADIF). Recents look only at the newest 2000
entries. Accuracy is shown only when the source reports it. A Bluetooth LE-only receiver with no serial port
is not supported. Builds are unsigned.

## V2 (2.0.0) — docs/EMCOMM_LOG_V2_Technical_Specification.md

Developed on macOS (Phases 1–4); Phase 5 re-enabled Windows/Linux CI. Final run on all three:
CI [41714457246](https://github.com/sam72128-PEDRO/emcomm-log/actions/runs/41714457246) green on Windows
and Linux, including the new independent ICS form validation (`scripts/validate_forms.py`, pypdf) and
Linux⇄Windows `.emlog` round trips; installed-app E2E
[41718785842](https://github.com/sam72128-PEDRO/emcomm-log/actions/runs/41718785842) **60/60 on Windows
(`.msi`) and 60/60 on Linux (`.deb`)** — the V1 checks (A01, A03, A07, A14) plus periods, channel plan,
PERIOD chip and Channel-field suggestion, axe on 12 screens/dialogs; installer smoke for all Windows/Linux
formats offline. macOS: local 2.0.0 `.dmg` passes `scripts/mac_smoke.sh`, all core/frontend tests pass, and
V1-era `.emlog` packages from Linux and Windows import and upgrade to schema 3.

| Phase / ID | Status | Evidence |
|---|---|---|
| Phase 1 — official forms gate | **passed** | FEMA ICS 213 v3 / ICS 214 v3.1 are AcroForm; filled and edited in macOS Preview and Chrome (DECISIONS.md #26, `docs/ICS_FIELD_MAP.md`). Acrobat Reader / Firefox not installed here — checked at Phase 5. |
| A16 Operational periods | **passing** | `forms::a16_periods_create_edit_void_overlap_and_current` (create/edit/void/restore, end ≤ start rejected, overlap reported, current period only while running, entries keep a voided period, resources persist). Release app: period created from the Dashboard, CURRENT badge, PERIOD chip in Quick Entry, entries tagged automatically. |
| A17 V1 upgrade | **passing** | `forms::a17_v1_logbook_and_package_upgrade_with_a_kept_copy` (verified pre-migration copy, free-text period untouched, V1 `.emlog` imports at schema 2). Release app: this morning's V1 test logbooks opened and upgraded with the notice naming the kept copy. |
| A18 ICS 213 | **passing** | `forms::a18_*` (every block, sequence order, skipped reasons, overflow to continuation pages, atomic write). Release app: Traffic Log selection → 2 forms, 1 entry listed as not a message; PDF checked field by field and visually in Preview. |
| A19 ICS 214 | **passing** | `forms::a19_*` (period/assignment selection, ordering, kinds, 24 + 36-row continuation pages with header repeated, >8 resources continue). Release app: 214 for OP 1 with the pre-period V1 entry correctly excluded. |
| A22 Viewers | **partial** | macOS Preview and Chrome: render, show Unicode (✓, José Núñez), fields editable. Windows and Linux: the forms are generated in CI and read back by pypdf (values, Unicode, AcroForm kept, no signatures). Not opened in Acrobat Reader or Firefox (not installed here; not available headless in CI) — worth one manual look. |
| A23 No invented signatures; footer | **passing** | Tests assert no `Signature*` field is ever filled; footer on every page (seen in Preview). |
| A24 Approval/reply persistence | **passing** | `forms::a24_*` (revisions, restart, `.emlog` round trip). |
| A20 ICS 309 | **passing** | `forms::a20_*` (period/kind scope, voided and out-of-period entries left out, selection narrowing with reasons, received/sent message numbers under FROM/TO, wrapped cells with no lost words, 44-line pages flowing to a second page). Sample PDF rendered with macOS Quick Look and checked visually. Release app: Traffic Log → ICS 309 for OP 1 (1 entry, the pre-period V1 message correctly left out) saved and its result banner fits. |
| A21/A25 channel plan | **passing** | `forms::a21_a25_*` (rows saved in order, blank rows dropped, N/W/mode/MHz normalised and validated with the row and field named, rejected saves change nothing, copy between periods, read-only copy from another logbook lists only periods with a plan, entries keep their saved channel text after plan edits, restart and `.emlog` round trip); migration 0003 up/down test; frontend test for the channel text. Release app (2.0.0): logbook upgraded 2 → 3 with its kept copy; ICS 205 typed in with Enter adding rows; an invalid TX frequency was refused with the row named and the cell focused; saved plan shown on the Dashboard; the Channel field suggested "TAC 2 (155.4750 MHz)" with its function/assignment and the entry saved with it and with OP 1. |

Hands-on V2 pass found and fixed: long upgrade notice overflowing its toast; Esc in a date picker closing the
whole dialog; activity start "defaults to event time" yet blocked saving (V1 bug); form-result buttons pushed
out of a narrow dialog; resources dialog not focusing its first cell; missing spaces in "From → To" and
"· n selected" (V1); the compact editor's Date/Time field overlapping the Type column (grid columns now
`minmax(0, 1fr)` — fixed in code, not yet seen on screen because the Mac locked). Also changed by design: the
"current period" is only a running one, so entries logged between periods are not attached to the last one.

## Other verification run

| Check | Result |
|---|---|
| `cargo test --workspace` | 64 passed (29 unit + 35 integration) on macOS, Windows and Linux; 5 opt-in helpers ignored |
| `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check` | clean |
| `npm run check` (svelte-check, fail on warnings), `npm run lint`, `npm run format:check` | clean |
| `npm test` (vitest) | 21 passed |
| Hands-on macOS pass (release build, 2026-11-03) | Every screen and flow driven by hand: wizard, Quick Entry (QSO + traffic), templates, drafts/restore, edit/void/revisions, Messages handling, Traffic Log, search, reports/exports, backups/restore, `.emlog` export/import, stale-lock recovery, close/quit. 23 issues found (validation shown as save failure, Cmd+Enter in a dialog saving the entry behind it, traffic entries storing QSO fields, no warning for unsaved drafts on close, stale counts, layout/wording) — all fixed and re-verified in the rebuilt app; regressions added to vitest |
| Scale (`--release --test scale -- --ignored`) | 100,000 entries: worst durable save 104 ms; FTS search 25 ms; filter page 176 ms; CSV 4.4 s, XLSX 6.3 s, DOCX 8.7 s, PDF 23 s, TXT 4.6 s |
| Migrations | up → down → up test; checksum tamper and newer-schema refusal tests |

## Installer artifacts (this machine)

See `release/SHA256SUMS-all.txt` (all verified after download). CI: Windows x64 `.msi`/`.exe`, Linux x86_64
`.deb`/`.rpm`/`.AppImage`; local build on this Mac: macOS arm64 `.dmg`. All unsigned and not notarized (no
credentials provided). Binaries are kept out of Git: Windows/Linux in the CI run's artifacts (copies under
`release/ci/`), macOS in `release/macos/`.

## Known limitations

- Unsigned builds (Gatekeeper/SmartScreen warnings). No notarization.
- PDF uses DejaVu Sans: CJK glyphs render as boxes in PDF (other formats are unaffected).
- XLSX truncates cells over 32,000 characters (Excel limit), with a marker and a warning.
- Network/cloud-folder detection is best-effort (mount type and well-known sync paths).
- Power-loss durability depends on the storage device honoring flushes; only process-kill was tested.
- Preset template editing in Settings is limited to removing templates or copying them from the open logbook.
