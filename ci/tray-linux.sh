#!/usr/bin/env bash
# Starts the tray app under Xvfb, waits for the window to close and 60s of idle, then records the
# memory of the app and its WebKit children and copies the app's phase0 report. Usage: tray-linux.sh <out-dir>
set -euo pipefail
out=$1
data="$HOME/.local/share/dev.prbots.phase0"

Xvfb :99 &
for _ in $(seq 30); do [[ -S /tmp/.X11-unix/X99 ]] && break; sleep 1; done
[[ -S /tmp/.X11-unix/X99 ]] || { echo "Xvfb did not start"; exit 1; }
export DISPLAY=:99
eval "$(dbus-launch --sh-syntax)"
# The keychain probe needs an unlocked Secret Service, as a desktop login gives.
eval "$(echo -n ci | gnome-keyring-daemon --unlock --components=secrets)"

./target/release/prbots-phase0 > "$out/tray-stdout.txt" 2>&1 &
app=$!

for _ in $(seq 120); do
  [[ "$(cat "$data/phase0-status" 2>/dev/null)" == done ]] && break
  kill -0 "$app" 2>/dev/null || { echo "tray app exited"; cat "$out/tray-stdout.txt"; exit 1; }
  sleep 1
done
[[ "$(cat "$data/phase0-status" 2>/dev/null)" == done ]] || { echo "no report after 120s"; exit 1; }
sleep 60

tree() { echo "$1"; for c in $(pgrep -P "$1"); do tree "$c"; done; }
{
  echo "pid name rss_kb pss_kb private_kb"
  for p in $(tree "$app"); do
    awk -v p="$p" -v n="$(cat /proc/$p/comm)" \
      '/^Rss:/{r=$2} /^Pss:/{s=$2} /^Private_(Clean|Dirty):/{v+=$2} END{print p, n, r, s, v}' "/proc/$p/smaps_rollup"
  done
} | tee "$out/memory.txt"
awk 'NR>1{s+=$4; v+=$5} END{printf "total pss %.1f MB, private %.1f MB\n", s/1024, v/1024}' "$out/memory.txt" \
  | tee -a "$out/memory.txt" >> "$GITHUB_STEP_SUMMARY"

cp "$data/phase0-report.json" "$out/phase0-tray.json"
kill "$app"
