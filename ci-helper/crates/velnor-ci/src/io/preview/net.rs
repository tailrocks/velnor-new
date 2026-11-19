// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;
use std::time::{Duration, Instant};

use serde_json::Value;
use ureq::http::Response;

use crate::error::CiError;
use crate::preview::authority::Client;
use crate::preview::json::{Res, parse, require};
use crate::preview::net::Net;
use crate::preview::{MAX_JSON, REPOSITORY};

const API: &str = "api.github.com";
const UPLOADS: &str = "uploads.github.com";
const RELEASE_HOSTS: [&str; 3] = [
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];
const TOTAL: Duration = Duration::from_secs(180);
const REQUEST: Duration = Duration::from_secs(30);
const REDIRECTS: usize = 10;
const ARTIFACT_HOPS: usize = 4;
const ARTIFACT_LIMIT: u64 = 257 * 1024 * 1024;
const USER_AGENT: &str = "velnor-source-preview";

/// The real network for the source preview helpers. A deadline of 180 seconds covers
/// everything it does. The token goes only to `api.github.com` and `uploads.github.com`.
#[derive(Debug)]
pub struct Transport {
    token: String,
    deadline: Instant,
    agent: ureq::Agent,
}

fn failure() -> CiError {
    CiError::new("source preview network failure")
}

/// The host of an `https://host[:443]/path` URL that has no user information or fragment.
fn split(url: &str) -> Res<&str> {
    let rest = url.strip_prefix("https://").ok_or_else(failure)?;
    let end = rest.find(['/', '?']).unwrap_or(rest.len());
    let authority = &rest[..end];
    require(!authority.contains('@') && !url.contains('#') && !authority.is_empty())?;
    let host = match authority.rsplit_once(':') {
        Some((host, "443")) => host,
        Some(_) => return Err(failure()),
        None => authority,
    };
    Ok(host)
}

impl Transport {
    #[must_use]
    pub fn new(token: String) -> Self {
        let agent = ureq::Agent::config_builder()
            .max_redirects(0)
            .http_status_as_error(false)
            .proxy(None)
            .build()
            .into();
        Self {
            token,
            deadline: Instant::now() + TOTAL,
            agent,
        }
    }

    fn remaining(&self) -> Res<Duration> {
        let left = self.deadline.saturating_duration_since(Instant::now());
        require(!left.is_zero())?;
        Ok(left.min(REQUEST))
    }

    fn headers(
        &self,
        request: ureq::RequestBuilder<ureq::typestate::WithoutBody>,
        signed: bool,
    ) -> ureq::RequestBuilder<ureq::typestate::WithoutBody> {
        let request = request
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", USER_AGENT)
            .header("X-GitHub-Api-Version", "2022-11-28");
        if signed && !self.token.is_empty() {
            request.header("Authorization", format!("Bearer {}", self.token))
        } else {
            request
        }
    }

    fn get(&self, url: &str, signed: bool) -> Res<Response<ureq::Body>> {
        let timeout = self.remaining()?;
        self.headers(self.agent.get(url), signed)
            .config()
            .timeout_global(Some(timeout))
            .build()
            .call()
            .map_err(|_| failure())
    }

    fn body(&self, mut response: Response<ureq::Body>, maximum: u64) -> Res<Vec<u8>> {
        if let Some(length) = response.headers().get("content-length") {
            let text = length.to_str().map_err(|_| failure())?;
            require(!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))?;
            require(text.parse::<u64>().is_ok_and(|n| n <= maximum))?;
        }
        let mut reader = response.body_mut().as_reader();
        let mut result = Vec::new();
        let mut chunk = vec![0_u8; 65_536];
        loop {
            require(Instant::now() <= self.deadline)?;
            let count = reader.read(&mut chunk).map_err(|_| failure())?;
            if count == 0 {
                break;
            }
            result.extend_from_slice(&chunk[..count]);
            require(result.len() as u64 <= maximum)?;
        }
        require(Instant::now() <= self.deadline)?;
        Ok(result)
    }

    fn location(response: &Response<ureq::Body>) -> Res<String> {
        let value = response.headers().get("location").ok_or_else(failure)?;
        Ok(value.to_str().map_err(|_| failure())?.to_owned())
    }

    fn json(&self, response: Response<ureq::Body>, allowed: &[u16]) -> Res<Value> {
        require(allowed.contains(&response.status().as_u16()))?;
        parse(&self.body(response, MAX_JSON as u64)?, MAX_JSON)
    }
}

impl Net for Transport {
    fn api(&self, path: &str) -> Res<Value> {
        let base = format!("/repos/{REPOSITORY}");
        require(path == base || path.starts_with(&format!("{base}/")))?;
        let response = self.get(&format!("https://{API}{path}"), true)?;
        self.json(response, &[200])
    }

    fn asset(&self, url: &str, maximum: usize) -> Res<Vec<u8>> {
        require(split(url)? == "github.com")?;
        let mut current = url.to_owned();
        for _ in 0..=REDIRECTS {
            let response = self.get(&current, false)?;
            let status = response.status().as_u16();
            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                let next = Self::location(&response)?;
                require(RELEASE_HOSTS.contains(&split(&next)?))?;
                current = next;
                continue;
            }
            require(status == 200)?;
            return self.body(response, maximum as u64);
        }
        Err(failure())
    }

    fn deadline(&self) -> Instant {
        self.deadline
    }
}

fn blob_host(host: &str) -> bool {
    host.strip_prefix("productionresultssa")
        .and_then(|rest| rest.strip_suffix(".blob.core.windows.net"))
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

impl Client for Transport {
    fn send(&self, method: &str, suffix: &str, body: &[u8], allowed: &[u16]) -> Res<Value> {
        let url = format!("https://{API}/repos/{REPOSITORY}{suffix}");
        self.write(method, &url, body, "application/json", allowed)
    }

    fn upload(&self, release_id: u64, name: &str, data: &[u8]) -> Res<Value> {
        let url = format!(
            "https://{UPLOADS}/repos/{REPOSITORY}/releases/{release_id}/assets?name={name}"
        );
        self.write("POST", &url, data, "application/octet-stream", &[201])
    }

    fn artifact_zip(&self, id: u64, size: u64, digest: &str) -> Res<Vec<u8>> {
        require(size > 0 && size <= ARTIFACT_LIMIT && digest.starts_with("sha256:"))?;
        let mut url = format!("https://{API}/repos/{REPOSITORY}/actions/artifacts/{id}/zip");
        let mut signed = true;
        for _ in 0..ARTIFACT_HOPS {
            let response = self.get(&url, signed)?;
            let status = response.status().as_u16();
            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                let next = Self::location(&response)?;
                require(blob_host(split(&next)?))?;
                url = next;
                signed = false;
                continue;
            }
            require(status == 200 && !signed)?;
            return self.body(response, ARTIFACT_LIMIT);
        }
        Err(CiError::new("preview artifact redirect limit"))
    }
}

impl Transport {
    fn write(
        &self,
        method: &str,
        url: &str,
        body: &[u8],
        content_type: &str,
        allowed: &[u16],
    ) -> Res<Value> {
        let host = split(url)?;
        require(host == API || host == UPLOADS)?;
        let timeout = self.remaining()?;
        let builder = match method {
            "POST" => self.agent.post(url),
            "PATCH" => self.agent.patch(url),
            _ => return Err(failure()),
        };
        let response = builder
            .header("Accept", "application/vnd.github+json")
            .header("Authorization", format!("Bearer {}", self.token))
            .header("User-Agent", USER_AGENT)
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("Content-Type", content_type)
            .config()
            .timeout_global(Some(timeout))
            .build()
            .send(body)
            .map_err(|_| failure())?;
        self.json(response, allowed)
    }
}
