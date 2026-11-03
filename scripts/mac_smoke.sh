#!/usr/bin/env bash
# Smoke-test a macOS .dmg locally (macOS is not built in CI): mount it, copy the app into a work
# folder (never /Applications), launch it with all IP networking denied and a scratch data folder,
# and check it stays up and creates its data store.
#
# Usage: scripts/mac_smoke.sh <path/to/EMCOMM LOG.dmg> <work-dir>
set -euo pipefail

dmg="$1"
work="$2"
mkdir -p "$work/data" "$work/app"
shasum -a 256 "$dmg"

mnt=$(hdiutil attach "$dmg" -nobrowse -readonly | tail -1 | awk -F'\t' '{print $NF}')
cp -R "$mnt/EMCOMM LOG.app" "$work/app/"
hdiutil detach "$mnt" >/dev/null
codesign --verify --deep --strict "$work/app/EMCOMM LOG.app" 2>/dev/null \
  && echo "signature: valid" || echo "signature: unsigned or ad-hoc"

# Seatbelt profile: everything allowed except IP networking (local IPC still works).
EMCOMM_DATA_DIR="$work/data" sandbox-exec \
  -p '(version 1)(allow default)(deny network-outbound (remote ip))(deny network-inbound (local ip))' \
  "$work/app/EMCOMM LOG.app/Contents/MacOS/emcomm-log" >"$work/dmg.log" 2>&1 &
pid=$!
sleep 20
screencapture -x "$work/dmg.png" || true
if kill -0 "$pid" 2>/dev/null; then
  echo "PASS app running with IP networking denied"
else
  cat "$work/dmg.log"
  echo "FAIL app exited"
  exit 1
fi
kill "$pid"
wait "$pid" 2>/dev/null || true
test -f "$work/data/app.sqlite" && echo "PASS data store created" || { echo "FAIL no app.sqlite"; exit 1; }
