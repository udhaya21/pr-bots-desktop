#!/usr/bin/env bash
# Idle memory of the phase0 tray app; run from a terminal inside a Linux desktop session.
# Usage: linux-desktop-memory.sh [path-to-prbots-phase0]  (default: the binary from the latest green CI run on main)
set -euo pipefail
repo=udhaya21/pr-bots-desktop
data="${XDG_DATA_HOME:-$HOME/.local/share}/dev.prbots.phase0"
out=$PWD/linux-desktop-memory.json
rm -f "$out"

[[ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]] || { echo "no DISPLAY or WAYLAND_DISPLAY: run this inside the desktop session"; exit 1; }
# A leftover copy shares library pages with the new one and makes its private numbers read low.
if pgrep -x prbots-phase0 >/dev/null; then echo "prbots-phase0 is already running; quit it first"; exit 1; fi

tree() { echo "$1"; for c in $(pgrep -P "$1"); do tree "$c"; done; }
app=
cleanup() {
  if [[ -n $app ]]; then
    mapfile -t pids < <(tree "$app")
    kill "${pids[@]}" 2>/dev/null || true
    rm -f "${XDG_RUNTIME_DIR:-/nonexistent}/tray-icon/tray-icon-$app-"*.png
    rmdir "${XDG_RUNTIME_DIR:-/nonexistent}/tray-icon" 2>/dev/null || true
  fi
  rm -rf "$tmp" "$data"
}
tmp=$(mktemp -d)
trap cleanup EXIT

bin=${1:-}
if [[ -z $bin ]]; then
  run=$(gh run list -R "$repo" -w phase0 -b main -s success -L 1 --json databaseId --jq '.[0].databaseId // empty')
  [[ -n $run ]] || { echo "no green phase0 run on main"; exit 1; }
  gh run download "$run" -R "$repo" -n prbots-phase0-linux-x86_64 -D "$tmp"
  bin=$tmp/prbots-phase0
  chmod +x "$bin"
fi
bin=$(realpath "$bin")
hint="needs x86_64 and: sudo apt install libwebkit2gtk-4.1-0 libayatana-appindicator3-1"
ldd "$bin" >"$tmp/ldd" 2>&1 || { echo "$bin is not a binary for this machine"; echo "$hint"; exit 1; }
if grep 'not found' "$tmp/ldd"; then echo "$hint"; exit 1; fi

cat <<'EOF'
The app will, as the Phase 0 probes do:
- run your login shell once to read PATH, and look up claude, codex, opencode and gh on it
- write, read and delete one Secret Service item (a locked keyring asks to unlock)
- if gh is logged in, read its token and make 2 read-only GitHub notifications requests
It opens a window for 5s, then stays in the tray. This script quits it and deletes its data when done.
EOF
read -rp "Enter to start, Ctrl-C to cancel " _ || true

rm -rf "$data"
"$bin" >"$tmp/app.log" 2>&1 &
app=$!

for _ in $(seq 120); do
  [[ "$(cat "$data/phase0-status" 2>/dev/null)" == "done" ]] && break
  kill -0 "$app" 2>/dev/null || { echo "the app exited early:"; tail -20 "$tmp/app.log"; echo "$hint"; exit 1; }
  sleep 1
done
[[ "$(cat "$data/phase0-status" 2>/dev/null)" == "done" ]] || { echo "the app did not finish its probes in 120s"; exit 1; }
echo "probes done, idling 60s"
sleep 60
kill -0 "$app" 2>/dev/null || { echo "the app exited during the idle minute:"; tail -20 "$tmp/app.log"; exit 1; }

json() { printf '%s' "$1" | tr '\n\t' '  ' | tr -d '\000-\037' | sed 's/\\/\\\\/g; s/"/\\"/g'; }
ours=$(tree "$app")
procs=()
dirty=0 private=0 swap=0
for p in $ours; do
  [[ -r /proc/$p/smaps_rollup ]] || continue
  read -r r s v d w < <(awk '/^Rss:/{r=$2} /^Pss:/{s=$2} /^Private_Dirty:/{d=$2} /^Private_(Clean|Dirty):/{v+=$2} /^Swap:/{w=$2}
    END{print r+0, s+0, v+0, d+0, w+0}' "/proc/$p/smaps_rollup")
  n=$(json "$(cat "/proc/$p/comm")")
  procs+=("{\"name\":\"$n\",\"rss_kb\":$r,\"pss_kb\":$s,\"private_kb\":$v,\"private_dirty_kb\":$d,\"swap_kb\":$w}")
  dirty=$((dirty + d)) private=$((private + v)) swap=$((swap + w))
done
(( ${#procs[@]} > 0 )) || { echo "could not read the app's memory"; exit 1; }
others=0
for p in $(pgrep WebKit || true); do grep -qx "$p" <<<"$ours" || others=$((others + 1)); done

gpu=$(lspci -k 2>/dev/null | grep -A3 -Ei 'vga|3d controller|display controller' | grep -Ei 'vga|3d|display|driver in use' \
  | sed 's/^[[:space:]]*//' | paste -sd'|' - || true)
renderer=$(glxinfo -B 2>/dev/null | grep -i 'renderer string' | cut -d: -f2- | sed 's/^ *//' || true)
[[ -n $renderer ]] || echo "no GL renderer line: install mesa-utils and rerun if you can"
# shellcheck source=/dev/null
os=$(. /etc/os-release && echo "$PRETTY_NAME")
proc_list=$(IFS=,; echo "${procs[*]}")

cat > "$out" <<EOF
{
  "os": "$(json "$os")",
  "kernel": "$(json "$(uname -r)")",
  "arch": "$(uname -m)",
  "session": "$(json "${XDG_SESSION_TYPE:-unknown}")",
  "desktop": "$(json "${XDG_CURRENT_DESKTOP:-unknown}")",
  "gpu": "$(json "$gpu")",
  "gl_renderer": "$(json "${renderer:-unknown}")",
  "other_webkit_processes": $others,
  "total_private_dirty_kb": $dirty,
  "total_private_kb": $private,
  "total_swap_kb": $swap,
  "processes": [$proc_list]
}
EOF
# phys_footprint on macOS also counts swapped and compressed pages, so the bar figure adds swap.
awk -v d="$dirty" -v v="$private" -v w="$swap" 'BEGIN{printf "private dirty + swap %.1f MB (bar < 80 MB); private %.1f MB\n", (d+w)/1024, v/1024}'
echo "wrote $out; send that file back"
