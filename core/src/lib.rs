pub mod engines;
pub mod env;
pub mod github;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Serialize;

use engines::{Ctx, Engine, EngineRun};

#[derive(Debug, Serialize)]
pub struct Report {
    pub inherited_path: String,
    pub path_source: String,
    pub clis: BTreeMap<&'static str, Option<String>>,
    pub keychain: Result<(), String>,
    pub notifications: Result<github::PollProbe, String>,
    pub engines: Vec<EngineRun>,
}

/// Runs every Phase 0 probe. `data` holds `fixture/` (built by fixtures/make-pr.sh) and gets scratch files.
pub fn run_phase0(data: &Path) -> Report {
    let inherited_path = std::env::var("PATH").unwrap_or_default();
    let (path, path_source) = env::resolve_path();
    let clis = ["claude", "codex", "opencode", "gh"]
        .into_iter()
        .map(|b| (b, env::find(b, &path).map(|p| p.display().to_string())))
        .collect();
    let keychain = keychain_roundtrip().map_err(|e| format!("{e:#}"));
    let notifications = github::gh_token(&path)
        .and_then(|t| github::notifications_probe(&t))
        .map_err(|e| format!("{e:#}"));
    let engines = run_engines(data, &path);
    Report { inherited_path, path_source, clis, keychain, notifications, engines }
}

fn keychain_roundtrip() -> Result<()> {
    let entry = keyring::Entry::new("pr-bots-desktop.phase0", "probe")?;
    let secret = format!("probe-{}", std::process::id());
    entry.set_password(&secret)?;
    let read = entry.get_password()?;
    entry.delete_credential()?;
    if read != secret {
        bail!("read back a different secret");
    }
    Ok(())
}

fn run_engines(data: &Path, path: &str) -> Vec<EngineRun> {
    let prepared = (|| -> Result<_> {
        let empty_gh = data.join("empty-gh");
        std::fs::create_dir_all(&empty_gh)?;
        let fixture = data.join("fixture");
        let diff = git_diff(&fixture)?;
        Ok((empty_gh, fixture, diff))
    })();
    let (empty_gh, fixture, diff) = match prepared {
        Ok(p) => p,
        Err(e) => {
            let error = Some(format!("setup: {e:#}"));
            return Engine::ALL
                .map(|engine| EngineRun { engine, schema_valid: false, result: None, error: error.clone(), secs: 0 })
                .into();
        }
    };
    let prompt = engines::prompt(&diff);
    std::thread::scope(|s| {
        let handles: Vec<_> = Engine::ALL
            .map(|engine| {
                let (empty_gh, fixture, prompt) = (&empty_gh, &fixture, &prompt);
                s.spawn(move || {
                    let scratch = data.join(format!("run-{}", engine.bin()));
                    let wt = scratch.join("wt");
                    let setup = copy_worktree(fixture, &wt).and_then(|()| engines::strip_pr_config(&wt));
                    if let Err(e) = setup {
                        let error = Some(format!("worktree: {e:#}"));
                        return EngineRun { engine, schema_valid: false, result: None, error, secs: 0 };
                    }
                    let ctx = Ctx { path, empty_gh, scratch: &scratch };
                    engines::run(engine, &ctx, &wt, prompt)
                })
            })
            .into();
        handles.into_iter().map(|h| h.join().expect("engine thread panicked")).collect()
    })
}

fn git_diff(repo: &Path) -> Result<String> {
    let out = env::run_bounded(
        Command::new("git").args(["diff", "HEAD~1", "HEAD"]).current_dir(repo),
        Duration::from_secs(30),
    )?;
    if !out.status.success() {
        bail!("git diff in {}: {}", repo.display(), String::from_utf8_lossy(&out.stderr));
    }
    String::from_utf8(out.stdout).context("diff is not UTF-8")
}

fn copy_worktree(from: &Path, to: &Path) -> Result<()> {
    if to.exists() {
        std::fs::remove_dir_all(to)?;
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    copy_dir(from, to).with_context(|| format!("copy {} to {}", from.display(), to.display()))
}

// Symlinks are refused, not copied: a PR could point one outside the worktree.
fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            bail!("symlink in worktree: {}", src.display());
        } else if kind.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_worktree_copies_nested_files_and_replaces_old_copy() {
        let root = std::env::temp_dir().join(format!("prbots-copy-{}", std::process::id()));
        let (src, dst) = (root.join("src"), root.join("out/wt"));
        std::fs::create_dir_all(src.join(".git/objects")).unwrap();
        std::fs::write(src.join(".git/objects/a"), "obj").unwrap();
        std::fs::write(src.join("top.ts"), "top").unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(dst.join("stale"), "x").unwrap();

        copy_worktree(&src, &dst).unwrap();

        assert_eq!(std::fs::read_to_string(dst.join(".git/objects/a")).unwrap(), "obj");
        assert_eq!(std::fs::read_to_string(dst.join("top.ts")).unwrap(), "top");
        assert!(!dst.join("stale").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn copy_worktree_refuses_symlinks() {
        let root = std::env::temp_dir().join(format!("prbots-link-{}", std::process::id()));
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::os::unix::fs::symlink("/etc/hosts", src.join("hosts")).unwrap();

        let err = copy_worktree(&src, &root.join("wt")).unwrap_err();

        assert!(format!("{err:#}").contains("symlink in worktree"));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
