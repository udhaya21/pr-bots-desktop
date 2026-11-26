# Phase 0 results

Date: 2026-11-26. Machine: macOS 27.2 (Apple silicon). The bar names macOS 14; nothing here ran on 14.
Decisions: Tauri 2 + Rust core, gh token first, conditional polls, subscription CLIs with a terms notice, manual publish.

## Bar (plan claim 6.1)

| Item | macOS | Windows (CI, Server 2025) | Ubuntu 24.04 (CI) |
|---|---|---|---|
| Tray app (no window while it runs; one opened and closed at start for the memory test), spawns claude/codex/opencode, each returns a schema-valid ReviewResult | claude PASS, opencode PASS, **codex BLOCKED** (ChatGPT usage limit until Nov 30) | tray app ran and wrote its report; engines NOT RUN (no CLIs or fixture in CI) | same as Windows |
| One secret saved and read through the OS keychain | PASS in one process (set, get, delete). Read across launches and signed builds not tested | PASS in one process, Credential Manager (tray app and terminal) | PASS in one process, Secret Service via an unlocked gnome-keyring (tray app). Terminal probe: no Secret Service, fails as expected |
| Idle memory with window closed < 80 MB | PASS: a webview window opened and closed, then 60s idle: app 54 MB phys_footprint plus a 4 MB WebKit Networking helper. RSS 108-138 MB across readings, mostly shared system frameworks | unclear: app process 4.3 MB private, 28 MB working set; no WebView2 process left in its tree. Whether the window rendered is unverified | **FAIL as measured**: app + WebKitNetworkProcess 92.5 MB private, 139.5 MB PSS, under Xvfb with no GPU (libEGL DRI3 warnings). Needs a real desktop to judge |
| Second Notifications call with If-None-Match returns 304, on 2 accounts | 1 account only: 200 then 304, rate limit unchanged. **No second account** | not run (no user token in CI) | not run (no user token in CI) |
| GUI launch finds the 3 CLIs | PASS: Finder launch inherits `/usr/bin:/bin:/usr/sbin:/sbin`, login-shell probe finds all 3 plus gh | partial: tray app inherits the runner PATH and finds gh; no CLIs installed; launched from a script, not Explorer | partial: login-shell probe finds gh; no CLIs installed; launched from a script, not a desktop session |
| Usage-limit output of claude and opencode captured | claude PARTIAL: real limit lines recorded by pr-bots in text mode; raw `--output-format json` output not captured. **opencode NOT CAPTURED** | n/a | n/a |

Phase 0 does not pass yet: codex, the second account, claude's JSON-mode limit, opencode's limit, and on Windows and Ubuntu: engines, GUI-launch CLI
discovery and the 304 item. Windows 11 itself and Linux memory on a real desktop are also open. CI evidence: run 45766070962 (artifacts
`phase0-windows-latest`, `phase0-ubuntu-24.04`). In CI the engine error reads `spawn "git"` because the fixture dir is
missing, not git.

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
- Second GitHub account for the 304 item.
- claude `--output-format json` limit output and opencode limit text: need real hits.
- Windows 11 and Ubuntu 24.04: `.github/workflows/phase0.yml` runs the probes, the tray app and the memory reading on
  `windows-latest` and `ubuntu-24.04`. `windows-latest` is Windows Server 2025, not Windows 11, so a pass there is
  evidence, not the bar. Xvfb has no tray host, so tray visibility on Linux stays unproven. CI installs no CLIs and
  builds no fixture, so the engine items stay open on both.
- Windows signing: SignPath Foundation shows no country rule; needs an OSI licence, public repo, CI-built
  binaries, MFA, and names the Foundation as publisher. Not yet applied.

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
