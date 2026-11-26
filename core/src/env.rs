use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

/// Runs a command with stdin closed and kills it after `limit`. Its own process group is killed
/// when it ends, so a leftover grandchild cannot hold the pipes open.
pub fn run_bounded(cmd: &mut Command, limit: Duration) -> Result<Output> {
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(cmd, 0);
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawn {:?}", cmd.get_program()))?;
    // Drain both pipes on threads, or a chatty child blocks on a full pipe.
    let drain = |r: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut r) = r {
                let _ = r.read_to_end(&mut buf);
            }
            buf
        })
    };
    let out = drain(child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
    let err = drain(child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            kill_group(child.id());
            let stdout = out.join().unwrap_or_default();
            let stderr = err.join().unwrap_or_default();
            return Ok(Output { status, stdout, stderr });
        }
        if started.elapsed() > limit {
            kill_group(child.id());
            let _ = child.kill();
            let _ = child.wait();
            bail!("timed out after {}s", limit.as_secs());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Not `/bin/kill -KILL -<pgid>`: procps-ng 4.0.4 (Ubuntu 24.04) reads `-17363` as `-1` and kills
/// every process of the user.
#[cfg(unix)]
fn kill_group(pgid: u32) {
    if let Ok(pgid @ 2..) = i32::try_from(pgid) {
        // SAFETY: killpg only sends a signal; pgid is a child we spawned as a group leader.
        unsafe { libc::killpg(pgid, libc::SIGKILL) };
    }
}

#[cfg(not(unix))]
fn kill_group(_: u32) {}

/// A GUI launch gets launchd's bare PATH, so ask the user's login shell for theirs,
/// then add the install dirs the three CLIs use in case the shell probe fails.
pub fn resolve_path() -> (String, String) {
    let inherited = std::env::var("PATH").unwrap_or_default();
    let (mut dirs, source) = match shell_path() {
        Ok(p) => (std::env::split_paths(&p).collect::<Vec<_>>(), "login-shell"),
        Err(_) => (std::env::split_paths(&inherited).collect(), "inherited"),
    };
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for d in [".local/bin", ".opencode/bin", ".bun/bin", ".npm-global/bin"] {
            dirs.push(home.join(d));
        }
    }
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"].map(PathBuf::from));
    dirs.dedup();
    let joined = std::env::join_paths(dirs).map(|p| p.to_string_lossy().into_owned());
    (joined.unwrap_or(inherited), source.to_string())
}

#[cfg(unix)]
fn shell_path() -> Result<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let out = run_bounded(
        Command::new(shell).args(["-l", "-i", "-c", r#"printf '__PATH__%s__PATH__' "$PATH""#]),
        Duration::from_secs(10),
    )?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.split("__PATH__")
        .nth(1)
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .context("no PATH in login shell output")
}

#[cfg(not(unix))]
fn shell_path() -> Result<String> {
    bail!("GUI apps inherit the user PATH on Windows")
}

pub fn find(bin: &str, path: &str) -> Option<PathBuf> {
    let names: Vec<String> = if cfg!(windows) {
        ["exe", "cmd", "bat"].iter().map(|e| format!("{bin}.{e}")).collect()
    } else {
        vec![bin.to_string()]
    };
    std::env::split_paths(path)
        .flat_map(|d| names.iter().map(move |n| d.join(n)))
        .find(|p| is_executable(p))
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

/// Keeps gh's token from an engine. An empty GH_CONFIG_DIR alone is not enough: gh still reads the
/// keychain. A placeholder GH_TOKEN takes precedence over it. A direct keychain read stays possible
/// for any engine with a shell.
pub fn engine_command(bin: &Path, path: &str, empty_gh: &Path, cwd: &Path) -> Command {
    let mut cmd = Command::new(bin);
    for var in ["GITHUB_TOKEN", "GH_ENTERPRISE_TOKEN", "GITHUB_ENTERPRISE_TOKEN"] {
        cmd.env_remove(var);
    }
    cmd.env("GH_TOKEN", "engine-has-no-github-access")
        .env("PATH", path)
        .env("GH_CONFIG_DIR", empty_gh)
        .current_dir(cwd);
    cmd
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn run_bounded_kills_the_grandchild_and_spares_the_caller() {
        let out = run_bounded(Command::new("sh").args(["-c", "sleep 30 & echo $!"]), Duration::from_secs(5)).unwrap();
        let pid: i32 = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap();
        // SAFETY: signal 0 only checks that the process exists.
        let alive = || unsafe { libc::kill(pid, 0) } == 0;
        let deadline = Instant::now() + Duration::from_secs(3);
        while alive() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!alive(), "grandchild {pid} survived");
    }
}
