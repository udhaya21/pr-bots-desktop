# Phase 0 results

Date: 2026-11-26. Machine: macOS 27.2 (Apple silicon). The bar names macOS 14; nothing here ran on 14.
Decisions: Tauri 2 + Rust core, gh token first, conditional polls, subscription CLIs with a terms notice, manual publish.

## Bar (plan claim 6.1)

| Item | macOS | Windows (CI, Server 2025) | Ubuntu 24.04 (CI) |
|---|---|---|---|
| Tray app (no window while it runs; one opened and closed at start for the memory test), spawns claude/codex/opencode, each returns a schema-valid ReviewResult | claude PASS, opencode PASS, **codex BLOCKED** (ChatGPT usage limit until Nov 30) | tray app ran and wrote its report; engines NOT RUN (no CLIs or fixture in CI) | same as Windows |
| One secret saved and read through the OS keychain | PASS in one process (set, get, delete). Read across launches and signed builds not tested | PASS in one process, Credential Manager (tray app and terminal) | PASS in one process, Secret Service via an unlocked gnome-keyring (tray app). Terminal probe: no Secret Service, fails as expected |
| Idle memory with window closed < 80 MB | PASS: a webview window opened and closed, then 60s idle: app 54 MB phys_footprint plus a 4 MB WebKit Networking helper. RSS 108-138 MB across readings, mostly shared system frameworks | unclear: app process 4.3 MB private, 28 MB working set; no WebView2 process left in its tree. Whether the window rendered is unverified | **not passed, unproven**: CI 92.5 MB private. Local VM: 83 MB private, of which 43 MB dirty; 61 MB with the DMA-BUF renderer off. Software GL only, no real desktop. See Linux memory below |
| Second Notifications call with If-None-Match returns 304 (one account; bar changed 2026-11-26) | PASS: 200 then 304, rate limit unchanged | not run (no user token in CI) | not run (no user token in CI) |
| GUI launch finds the 3 CLIs | PASS: Finder launch inherits `/usr/bin:/bin:/usr/sbin:/sbin`, login-shell probe finds all 3 plus gh | partial: tray app inherits the runner PATH and finds gh; no CLIs installed; launched from a script, not Explorer | partial: login-shell probe finds gh; no CLIs installed; launched from a script, not a desktop session |
| Usage-limit output of claude and opencode captured | claude PARTIAL: real limit lines recorded by pr-bots in text mode; raw `--output-format json` output not captured. **opencode NOT CAPTURED** | n/a | n/a |

Phase 0 does not pass yet: codex, claude's JSON-mode limit, opencode's limit, and on Windows and Ubuntu: engines, GUI-launch CLI
discovery and the 304 item. Windows 11 itself and Linux memory on a real desktop are also open. CI evidence: run 45766070962 (artifacts
`phase0-windows-latest`, `phase0-ubuntu-24.04`). In CI the engine error reads `spawn "git"` because the fixture dir is
missing, not git.

## Bar changes

- 304 item, 2026-11-26: one account instead of 2. The server answers 304 by comparing the ETag the client sends,
  so it is per request, not per account, and no second account is available.

## Linux memory

Ubuntu 24.04 arm64 VM (lima, Apple Virtualization), Xvfb, Mesa llvmpipe (no GPU), release build. App plus
WebKitNetworkProcess, read from `/proc/<pid>/smaps_rollup` 60s after the probes finished; at 180s the default read
1 MB higher and the others the same. Default, DMA-BUF off and no-window ran twice (within 1.1 MB); the rest once.
The malloc_trim row was read at 180s.

| Variant | Private (USS) | Private dirty | PSS |
|---|---|---|---|
| Default (window opened and closed) | 83.2 MB | 43.0 MB | 132.5 MB |
| Default, then `malloc_trim(0)` via gdb | 69.3 MB | 29.2 MB | 118.7 MB |
| `WEBKIT_DISABLE_DMABUF_RENDERER=1` | 61.0 MB | 28.9 MB | 110.3 MB |
| `WEBKIT_DISABLE_COMPOSITING_MODE=1` | 77.9 MB | not read | 127.6 MB |
| `MALLOC_ARENA_MAX=2` | 82.4 MB | not read | 132.2 MB |
| Window never opened (no network process) | 54.2 MB | 20.2 MB | 75.8 MB |
| Wayland (weston headless) | 142.1 MB | not read | 172.6 MB |

- About 40 MB of the default USS is clean file pages (USS minus dirty). The largest private file mappings are
  libwebkit2gtk 10 MB and Mesa's software renderer, libgallium 10.5 MB plus libLLVM 10 MB; the per-library clean/dirty
  split was not read. The kernel can drop clean pages under pressure. A real GPU
  driver replaces the Mesa ones with its own, so this VM cannot say what a desktop shows.
- Freed heap is the other lever: `malloc_trim(0)` after the window closed returned 14 MB of dirty heap.
- Headless Wayland is worse (libLLVM 48 MB private), so Xvfb is not the reason CI read high.
- The bar does not name a Linux metric. macOS passed on phys_footprint, which counts dirty memory, not clean pages;
  the Linux counterpart is private dirty (43 MB). Plain `ps` RSS is 198 MB here and also failed on macOS.
- Still needed: a reading on a real Ubuntu desktop with a GPU, and a decision on the metric.

## Findings

1. **`OPENCODE_CONFIG` does not lock opencode down.** With a deny config in `OPENCODE_CONFIG` and a PR `opencode.json`
   that allows `shell`, the model ran the shell command. With `OPENCODE_CONFIG_CONTENT` the model listed its tools
   and had no shell. One run each. The core uses `OPENCODE_CONFIG_CONTENT` and also deletes the PR's agent config,
   since CONTENT has not been tested against PR `mcp`, `plugin` or `provider` keys.
2. **opencode v2.2.2 names its tool `shell`**, not `bash` (seen in pr-bots logs). The deny config sets both.
3. **The claude lock-down holds.** Control run in the hostile fixture without the lock flags: the PR's SessionStart
   hook, PreToolUse hook and MCP server all ran. With `--setting-sources user --strict-mcp-config
   --settings {"disableAllHooks":true} --tools Read,Grep,Glob`, the model made a Read call (2 turns) and none ran.
4. **An empty `GH_CONFIG_DIR` does not hide the gh token.** `gh auth token` still returned the keychain token while
   `gh auth status` said not logged in. Plan claim 4.1 is wrong. A placeholder `GH_TOKEN` shadows it for gh. Any
   engine with a shell can still read the keychain directly (`security find-generic-password`); codex is the one to test.
5. **codex reads stdin** ("Reading additional input from stdin..."); the core closes stdin on every spawn.
6. **`open` from a terminal is not a GUI test**: the app inherited the terminal PATH. A Finder launch gave launchd's bare PATH.
7. Both working engines flagged the hostile config in their review and said they did not act on it.
8. **`--setting-sources user` also drops the project CLAUDE.md.** Asked for the first heading of the repo's CLAUDE.md,
   claude answered NONE with the flag and quoted it without (a private repo, one run each). The core's lock-down
   therefore runs the review without the repo's own conventions; pass them in the prompt if reviews need them.
9. **`/bin/kill -KILL -<pgid>` killed every process of the user on Ubuntu 24.04.** procps-ng 4.0.4 parses `-17363`
   as `-1` (strace: `-999999` became `kill(-9, SIGKILL)`). In a VM the tray app's first spawn ended the whole login
   session. CI passed by luck of its pid values. The core now calls `killpg`, with a test that a grandchild dies and
   the test process lives.

## Usage-limit text captured

- claude (text mode, exit 1, as pr-bots recorded it): `You've hit your session limit · resets 5:30pm (<tz>)`,
  also `5-hour limit · resets 12:50pm` and `weekly limit · resets Nov 1 at 5:30pm`.
- codex (`--json`, exit 1): `{"type":"error","message":"You’ve hit your usage limit. ... try again at Nov 30th, 2026 11:28 AM."}`
  then `turn.failed` with the same message.
- opencode: none.

## Known gaps in the spike code (Phase 1)

- Prompt and diff go in argv; a large PR hits ARG_MAX. Pass them on stdin.
- `AGENTS.md` and `CLAUDE.md` stay in the worktree. They are prompt text, not engine config; decide whether to drop them.
- Only the GitHub token vars are scrubbed; other secrets in the user's env reach the engine. `GH_ENTERPRISE_TOKEN` has no placeholder, so GHES likely still falls back to the keychain.
- Engines run in their own process group: Ctrl-C on `phase0` or Quit mid-run leaves them running. A grandchild that calls `setsid` escapes the group kill and can hang the pipe drain.
- The run guard is not reset if the probe thread panics.

## Open

- codex: ReviewResult spawn, `-s read-only` network test, and a direct keychain read under its sandbox, after Nov 30.
- claude `--output-format json` limit output and opencode limit text: need real hits.
- Windows 11 and Ubuntu 24.04: `.github/workflows/phase0.yml` runs the probes, the tray app and the memory reading on
  `windows-latest` and `ubuntu-24.04`. `windows-latest` is Windows Server 2025, not Windows 11, so a pass there is
  evidence, not the bar. Xvfb has no tray host, so tray visibility on Linux stays unproven. CI installs no CLIs and
  builds no fixture, so the engine items stay open on both.
- Windows signing: SignPath Foundation shows no country rule; needs an OSI licence, public repo, CI-built
  binaries, MFA, and names the Foundation as publisher. Licence is now MIT OR Apache-2.0 (both OSI). Not yet applied.

## Run it

```sh
fixtures/make-pr.sh "$HOME/Library/Application Support/dev.prbots.phase0/fixture" /tmp/marks
cd app && cargo tauri build --bundles app
# GUI launch (Finder, not `open` from a terminal):
osascript -e 'tell application "Finder" to open POSIX file "'$PWD'/../target/release/bundle/macos/pr-bots-phase0.app"'
# report: ~/Library/Application Support/dev.prbots.phase0/phase0-report.json
cargo run -p prbots-core --bin phase0 -- <data-dir>   # same probes from a terminal
experiments/lockdown.sh <out-dir>                     # hostile-config experiments
```
