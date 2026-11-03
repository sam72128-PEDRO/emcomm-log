# EMCOMM LOG 2.0

Offline, single-user desktop communications log for routine logging, amateur radio (HAM), and emergency
activations. Windows, macOS, and Linux. Tauri 2 + Svelte 5/TypeScript UI, Rust core, SQLite storage.

- Every **Save Entry / Save Changes** commits directly to the logbook's SQLite database in the folder you
  chose (WAL + `synchronous=FULL`, verified on open). There is no "Save Logbook" step.
- Unfinished entries are autosaved as **drafts** (~2 s after typing stops) in the local app-data folder and
  offered for recovery after a crash.
- Independent, verified **backups** (automatic interval, on close, manual) with retention, and **restore**
  as a separate logbook.
- Portable **`.emlog`** packages for moving a complete logbook between computers.
- **Reports**: DOCX, XLSX, PDF, CSV, TXT, and ADIF 3.1.4 (qualifying HAM QSOs). No Office required.
- **ICS forms** (emergency logbooks, V2): operational periods with assigned resources; ICS 213 General
  Message and ICS 214 Activity Log filled on FEMA's official PDFs; ICS 309 Communications Log; a channel
  plan per period (typed in from an ICS 205) that feeds the Channel field. Signatures are always left blank.
- No network use, accounts, cloud sync, or telemetry.

Documentation: [Operator guide](docs/OPERATOR_GUIDE.md) · [Export formats](docs/EXPORT_FORMATS.md) ·
[Decisions](DECISIONS.md) · [Implementation status / acceptance matrix](IMPLEMENTATION_STATUS.md) ·
[V1 spec](docs/) · [V2 spec](docs/EMCOMM_LOG_V2_Technical_Specification.md) · [ICS field map](docs/ICS_FIELD_MAP.md)

## Repository layout

| Path | Contents |
|---|---|
| `core/` | `emcomm-core` Rust crate: schema/migrations, live logbook + lock, entries/revisions, drafts, backups/restore, `.emlog`, exports, validation. All authoritative logic. |
| `core/migrations/` | Versioned SQL migrations (`*.up.sql` / `*.down.sql`), checksummed at apply time. |
| `core/tests/` | Integration tests against real SQLite: durability, process-kill, concurrency, fault injection, backups, packages (incl. malicious archives), exports, scale. |
| `src-tauri/` | Tauri shell: typed commands, native menus, Rust-side file dialogs, backup scheduler, close workflow. |
| `src/` | Svelte UI (`screens/`, `components/`, `lib/`), frontend tests in `src/test/`. |
| `scripts/validate_exports.py` | Independent validation of export fixtures with python-docx, openpyxl, pypdf, adif-io. |
| `.github/workflows/ci.yml` | Native Windows and Linux CI: lint, typecheck, tests, fixture validation, installers, Linux⇄Windows `.emlog` round trip. `installer-smoke.yml` then installs and E2E-tests those installers. macOS is built and tested locally (below). |
| `scripts/mac_smoke.sh` | Local macOS installer smoke test: mounts the `.dmg`, runs the app with IP networking denied. |

## Prerequisites

All platforms: **Node.js 22+**, **Rust stable (1.89+; built with 1.99.1)** via [rustup](https://rustup.rs).

| OS | Additional prerequisites |
|---|---|
| macOS 11+ | Xcode Command Line Tools (`xcode-select --install`). |
| Windows 10/11 | Microsoft C++ Build Tools ("Desktop development with C++"), WebView2 runtime (preinstalled on Windows 11). |
| Linux (Debian/Ubuntu 22.04+) | `sudo apt install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libssl-dev libxdo-dev build-essential file` |

Dependency versions are pinned in `package-lock.json` and `Cargo.lock`. The Tauri npm packages
(`@tauri-apps/api`, `@tauri-apps/cli`) must stay on the same minor version as the `tauri` crate.

## Develop, test, build

```bash
npm ci
npm run tauri dev          # run the app with hot reload
```

```bash
npm run format:check && npm run lint && npm run check && npm test     # frontend
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                                                 # Rust unit + integration
```

Independent export validation:

```bash
python3 -m venv .venv-validate && .venv-validate/bin/pip install -r scripts/requirements-validate.txt
EMCOMM_FIXTURE_OUT=/tmp/emcomm-fixtures cargo test -p emcomm-core --test exports write_fixture
.venv-validate/bin/python scripts/validate_exports.py /tmp/emcomm-fixtures
```

100,000-entry scale test (several minutes):

```bash
cargo test -p emcomm-core --release --test scale -- --ignored --nocapture
```

Installers (built on each target OS; cross-compiling installers is not supported):

```bash
npx tauri build
```

Output goes to `target/release/bundle/` — macOS `.app` + `.dmg`, Windows `.msi` + NSIS `-setup.exe`,
Linux `.deb`, `.rpm`, `.AppImage`.

### Building on macOS

CI builds only Windows and Linux; the macOS release is built on a Mac:

```bash
npm ci && npx tauri build                      # -> target/release/bundle/dmg/*.dmg
scripts/mac_smoke.sh target/release/bundle/dmg/*.dmg /tmp/emcomm-mac-smoke
shasum -a 256 target/release/bundle/dmg/*.dmg  # publish next to the .dmg
```

To check a Windows/Linux `.emlog` on the Mac, download a CI `emlog-fixture-*` artifact and run
`EMCOMM_IMPORT_PACKAGE=<file.emlog> cargo test -p emcomm-core --test crossos -- --ignored`.

`EMCOMM_DATA_DIR=<folder>` relocates the local app-data folder (settings, profiles, presets, drafts),
for testing or portable use.

## Signing

Builds are **unsigned**. macOS will require right-click → Open the first time (Gatekeeper); Windows
SmartScreen may warn. Code signing and macOS notarization need credentials that were not provided; see
[docs/SIGNING.md](docs/SIGNING.md).
