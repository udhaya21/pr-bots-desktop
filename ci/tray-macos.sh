#!/usr/bin/env bash
# Starts the tray app, waits for the window to close and 60s of idle, then records phys_footprint of the app
# and the WebKit helpers (launchd starts those, not the app), and copies the app's phase0 report.
# Usage: tray-macos.sh <out-dir>
set -euo pipefail
out=$1
data="$HOME/Library/Application Support/dev.prbots.phase0"

./target/release/prbots-phase0 > "$out/tray-stdout.txt" 2>&1 &
app=$!

for _ in $(seq 120); do
  [[ "$(cat "$data/phase0-status" 2>/dev/null)" == done ]] && break
  kill -0 "$app" 2>/dev/null || { echo "tray app exited"; cat "$out/tray-stdout.txt"; exit 1; }
  sleep 1
done
[[ "$(cat "$data/phase0-status" 2>/dev/null)" == done ]] || { echo "no report after 120s"; exit 1; }
sleep 60

sw_vers | tee "$out/memory.txt"
pids=("$app" $(pgrep -f 'com\.apple\.WebKit\.' || true))
footprint --noCategories "${pids[@]}" | tee -a "$out/memory.txt"
grep -q "\[$app\]" "$out/memory.txt" || { echo "no footprint for the app"; exit 1; }
grep 'Footprint:' "$out/memory.txt" >> "$GITHUB_STEP_SUMMARY"

cp "$data/phase0-report.json" "$out/phase0-tray.json"
kill "$app"
