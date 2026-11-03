# Implementation status (acceptance matrix A01–A15)

Environments: local macOS 14.6.1 (Apple Silicon), Rust 1.99.1, Node 26; plus GitHub Actions native runners
**windows-latest, ubuntu-22.04, macos-latest** (private repo `sam72128-PEDRO/emcomm-log`). CI run
[41587742360](https://github.com/sam72128-PEDRO/emcomm-log/actions/runs/41587742360): all 9 jobs green on
all three OSs (format, lint, typecheck, frontend tests, clippy `-D warnings`, all Rust unit/integration tests,
independent export validation, installer builds, cross-OS `.emlog` round trips).

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
| A10 `.emlog` on another OS preserves data; malformed/oversized/traversal/duplicate safe | **passing** | CI cross-OS round trips Linux→Windows, Windows→macOS, macOS→Linux pass (`crossos.rs`: Unicode names, attachments+hashes, revisions, void, favorites, templates, logbook settings). | Same-OS: `protection::a10_a13_package_round_trip_preserves_everything` (all 3 types; entries, revisions, templates, favorites, settings, attachments, Unicode names; duplicate import reported, never overwrites), `crossos::same_os_round_trip_matches_crossos_expectations`. Malformed: traversal, absolute, backslash, unexpected file, WAL sidecar, missing manifest, future format/schema, wrong format, hash mismatch, malformed manifest, non-SQLite DB, symlink, duplicate names, truncated archive — all rejected with no leftovers. Oversize limits are enforced in code but no >4 GB fixture was built. |
| A11 DOCX/XLSX/PDF/CSV/TXT match scope, order, timestamps, void, long text; no Office | **passing** | Fixtures generated and validated by independent parsers on Windows, Linux and macOS in CI. | `exports::a11_*` and `scripts/validate_exports.py`: 43/43 independent checks pass (csv, openpyxl, python-docx, pypdf). PDF rendered with macOS Quick Look. Release app DOCX export read back with python-docx. 100k-entry streaming exports in `scale.rs`. Not opened in desktop viewers (Word/LibreOffice/Acrobat) on Windows/Linux. |
| A12 ADIF only eligible QSOs, valid lengths/dates, skipped report | **passing** | `exports::a12_adif_contains_only_eligible_qsos_with_valid_fields` (independent ADI reader checks byte lengths), adif-io parses the fixture (4 QSOs, UTC times). |
| A13 Emergency message/activity fields persist & round trip; not presented as official | **passing** | `protection::a10_a13_*` (message & activity fields, incident snapshot), `exports::emergency_exports_do_not_claim_official_forms`; UI labels say "not an official form". |
| A14 Menus/shortcuts on all OSs; focus, Tab, Esc, screen-reader status | **partial** | Frontend shortcut/dialog tests pass on all 3 OSs in CI. | macOS release app: native menus with shortcuts; Cmd+2/3/E/Enter, Tab order, Esc-close verified by hand. Frontend tests: key mapping, Ctrl/Cmd+Enter save, shortcuts blocked in dialogs with announcement. Live regions and labelled fields are implemented. Not done: hand-testing the native menus on Windows/Linux, and a VoiceOver/NVDA/Orca pass. |
| A15 Native installers launch offline on clean machine; no placeholders/demo data | **partial** | All installers build on native runners: Windows `.msi` + NSIS `-setup.exe`, Linux `.deb` + `.rpm` + `.AppImage`, macOS `.dmg` (checksums in `release/SHA256SUMS-all.txt`). The macOS build was launched and used locally. **Not done:** installing and launching each on a clean, offline machine (needs Windows/Linux hardware or VMs). No demo data is shipped. |

## Other verification run

| Check | Result |
|---|---|
| `cargo test --workspace` | 64 passed (29 unit + 35 integration) on macOS, Windows and Linux; 5 opt-in helpers ignored |
| `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check` | clean |
| `npm run check` (svelte-check, fail on warnings), `npm run lint`, `npm run format:check` | clean |
| `npm test` (vitest) | 13 passed |
| Scale (`--release --test scale -- --ignored`) | 100,000 entries: worst durable save 104 ms; FTS search 25 ms; filter page 176 ms; CSV 4.4 s, XLSX 6.3 s, DOCX 8.7 s, PDF 23 s, TXT 4.6 s |
| Migrations | up → down → up test; checksum tamper and newer-schema refusal tests |

## Installer artifacts (this machine)

See `release/SHA256SUMS-all.txt` (all verified after download). CI: Windows x64 `.msi`/`.exe`, Linux x86_64
`.deb`/`.rpm`/`.AppImage`, macOS arm64 `.dmg`; local: macOS arm64 `.dmg`. All unsigned and not notarized (no
credentials provided). Binaries are kept out of Git; download them from the CI run's artifacts.

## Known limitations

- Unsigned builds (Gatekeeper/SmartScreen warnings). No notarization.
- PDF uses DejaVu Sans: CJK glyphs render as boxes in PDF (other formats are unaffected).
- XLSX truncates cells over 32,000 characters (Excel limit), with a marker and a warning.
- Network/cloud-folder detection is best-effort (mount type and well-known sync paths).
- Power-loss durability depends on the storage device honoring flushes; only process-kill was tested.
- Preset template editing in Settings is limited to removing templates or copying them from the open logbook.
