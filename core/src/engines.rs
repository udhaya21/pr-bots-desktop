use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use serde_json::Value;

use crate::env;

pub const SCHEMA: &str = include_str!("review_result.schema.json");
const LIMIT: Duration = Duration::from_secs(600);
const OPENCODE_DENY: &str = r#"{"$schema":"https://opencode.ai/config.json","permission":{"shell":"deny","bash":"deny","edit":"deny","webfetch":"deny"}}"#;
// Agent config a PR author controls. Removed from the worktree before any engine runs.
const PR_AGENT_CONFIG: [&str; 6] = [".claude", ".mcp.json", ".opencode", "opencode.json", "opencode.jsonc", ".codex"];

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Claude,
    Codex,
    Opencode,
}

impl Engine {
    pub const ALL: [Engine; 3] = [Engine::Claude, Engine::Codex, Engine::Opencode];

    pub fn bin(self) -> &'static str {
        match self {
            Engine::Claude => "claude",
            Engine::Codex => "codex",
            Engine::Opencode => "opencode",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct EngineRun {
    pub engine: Engine,
    pub schema_valid: bool,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub secs: u64,
}

pub struct Ctx<'a> {
    pub path: &'a str,
    pub empty_gh: &'a Path,
    pub scratch: &'a Path,
}

pub fn strip_pr_config(worktree: &Path) -> Result<()> {
    for name in PR_AGENT_CONFIG {
        let p = worktree.join(name);
        if p.is_dir() {
            std::fs::remove_dir_all(&p)?;
        } else if p.exists() {
            std::fs::remove_file(&p)?;
        }
    }
    Ok(())
}

pub fn prompt(diff: &str) -> String {
    format!(
        "Review this pull request. The working directory is the PR head; read files if you need context.\n\
         Report real bugs only. Each comment's line is a line number in the head version of the file.\n\
         Reply with only a JSON object that matches this JSON Schema:\n{SCHEMA}\n\nDiff:\n```diff\n{diff}\n```"
    )
}

pub fn run(engine: Engine, ctx: &Ctx, worktree: &Path, prompt: &str) -> EngineRun {
    let started = Instant::now();
    let outcome = env::find(engine.bin(), ctx.path)
        .ok_or_else(|| anyhow!("{} not on PATH", engine.bin()))
        .and_then(|bin| invoke(engine, &bin, ctx, worktree, prompt));
    let (result, error) = match outcome {
        Ok(v) => (Some(v), None),
        Err(e) => (None, Some(format!("{e:#}"))),
    };
    let schema_valid = result.as_ref().is_some_and(valid);
    EngineRun { engine, schema_valid, result, error, secs: started.elapsed().as_secs() }
}

pub fn valid(v: &Value) -> bool {
    let schema: Value = serde_json::from_str(SCHEMA).expect("schema is JSON");
    jsonschema::validator_for(&schema).expect("schema compiles").is_valid(v)
}

fn invoke(engine: Engine, bin: &Path, ctx: &Ctx, wt: &Path, prompt: &str) -> Result<Value> {
    let mut cmd = env::engine_command(bin, ctx.path, ctx.empty_gh, wt);
    match engine {
        Engine::Claude => {
            cmd.args(["-p", prompt, "--output-format", "json", "--json-schema", SCHEMA])
                .args(["--tools", "Read,Grep,Glob", "--permission-mode", "dontAsk"])
                .args(["--setting-sources", "user", "--strict-mcp-config"])
                .args(["--settings", r#"{"disableAllHooks":true,"permissions":{"blockReadsOutsideWorkingDirectories":true}}"#])
                .args(["--model", "sonnet", "--max-budget-usd", "1", "--no-session-persistence"]);
            let out = checked(env::run_bounded(&mut cmd, LIMIT)?)?;
            let v: Value = serde_json::from_slice(&out).context("claude stdout is not JSON")?;
            if v["is_error"] == true {
                bail!("claude error: {}", v["result"]);
            }
            v.get("structured_output").cloned().context("no structured_output")
        }
        Engine::Codex => {
            let schema = ctx.scratch.join("codex-schema.json");
            let last = ctx.scratch.join("codex-last.txt");
            std::fs::write(&schema, SCHEMA)?;
            let _ = std::fs::remove_file(&last);
            cmd.args(["exec", prompt, "-s", "read-only", "--ignore-user-config", "--ephemeral"])
                .arg("--output-schema")
                .arg(&schema)
                .arg("-o")
                .arg(&last);
            checked(env::run_bounded(&mut cmd, LIMIT)?)?;
            parse_json_text(&std::fs::read_to_string(&last)?)
        }
        Engine::Opencode => {
            // OPENCODE_CONFIG loses to a PR's opencode.json; OPENCODE_CONFIG_CONTENT is applied last.
            cmd.env("OPENCODE_CONFIG_CONTENT", OPENCODE_DENY)
                .args(["run", prompt, "--standalone", "--format", "json"])
                .args(["--model", "opencode-go/deepseek-v4.1-flash"]);
            let out = checked(env::run_bounded(&mut cmd, LIMIT)?)?;
            let text = String::from_utf8_lossy(&out)
                .lines()
                .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                .filter(|e| e["type"] == "text")
                .filter_map(|e| e["part"]["text"].as_str().map(str::to_owned))
                .next_back()
                .context("opencode printed no text event")?;
            parse_json_text(&text)
        }
    }
}

fn checked(out: std::process::Output) -> Result<Vec<u8>> {
    if out.status.success() {
        return Ok(out.stdout);
    }
    let tail = |b: &[u8]| {
        let s = String::from_utf8_lossy(b);
        s.chars().rev().take(600).collect::<Vec<_>>().into_iter().rev().collect::<String>()
    };
    bail!("exit {}: stderr: {} stdout: {}", out.status, tail(&out.stderr), tail(&out.stdout))
}

/// Model text may wrap the object in a code fence; take the outermost braces.
fn parse_json_text(text: &str) -> Result<Value> {
    let start = text.find('{').context("no JSON object in reply")?;
    let end = text.rfind('}').context("no JSON object in reply")?;
    serde_json::from_str(&text[start..=end]).context("reply is not valid JSON")
}
