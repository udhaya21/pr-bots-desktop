use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::blocking::{Client, Response};
use serde::Serialize;

use crate::env;

#[derive(Debug, Serialize)]
pub struct PollProbe {
    pub first_status: u16,
    pub etag: Option<String>,
    pub poll_interval: Option<String>,
    pub second_status: u16,
    pub rate_remaining: [Option<String>; 2],
}

/// Decision 2: the gh token is the first identity.
pub fn gh_token(path: &str) -> Result<String> {
    let gh = env::find("gh", path).context("gh not on PATH")?;
    let out = env::run_bounded(
        Command::new(gh).args(["auth", "token"]).env("PATH", path),
        Duration::from_secs(15),
    )?;
    if !out.status.success() {
        bail!("gh auth token: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8(out.stdout)?.trim().to_owned())
}

/// Two Notifications calls; the second sends the first ETag and should get 304, which costs no rate limit.
pub fn notifications_probe(token: &str) -> Result<PollProbe> {
    let client = Client::builder().user_agent("pr-bots-desktop-phase0").build()?;
    let get = |etag: Option<&str>| -> Result<Response> {
        let mut req = client
            .get("https://api.github.com/notifications")
            .bearer_auth(token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(tag) = etag {
            req = req.header("If-None-Match", tag);
        }
        Ok(req.send()?)
    };
    let header = |r: &Response, name: &str| r.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_owned);

    let first = get(None)?;
    let etag = header(&first, "etag");
    let poll_interval = header(&first, "x-poll-interval");
    let rate_first = header(&first, "x-ratelimit-remaining");
    let first_status = first.status().as_u16();
    let second = get(etag.as_deref())?;
    Ok(PollProbe {
        first_status,
        etag,
        poll_interval,
        second_status: second.status().as_u16(),
        rate_remaining: [rate_first, header(&second, "x-ratelimit-remaining")],
    })
}
