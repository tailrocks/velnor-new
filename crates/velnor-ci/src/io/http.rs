// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;
use std::time::Duration;

use ureq::RequestBuilder;
use ureq::http::{Response, StatusCode};

use crate::error::CiError;
use crate::github::{GitHub, GitHubWrite};
use crate::release_family::GitHubPost;

pub(super) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);
const BODY_LIMIT: u64 = 512 * 1024 * 1024;
const ERROR_BODY_LIMIT: u64 = 64 * 1024;
const DETAIL_LIMIT: usize = 500;
const RESPONSE_HEADERS: [&str; 3] = ["retry-after", "x-ratelimit-remaining", "x-ratelimit-reset"];

#[derive(Debug)]
pub(super) struct HttpGitHub {
    agent: ureq::Agent,
    api: String,
    token: String,
}

impl HttpGitHub {
    pub(super) fn new(api: String, token: String, timeout: Duration) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(crate::trusted_roots::configured_roots())
                    .build(),
            )
            .build()
            .into();
        Self { agent, api, token }
    }

    fn signed<B>(&self, request: RequestBuilder<B>) -> RequestBuilder<B> {
        request
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "velnor-ci")
    }
}

fn body(
    what: &str,
    response: Result<Response<ureq::Body>, ureq::Error>,
) -> Result<String, CiError> {
    let mut response = response.map_err(|e| CiError::new(format!("{what}: {e}")))?;
    if is_failure(response.status()) {
        return Err(status_error(what, &mut response));
    }
    let bytes = super::read_limited(response.body_mut().as_reader(), BODY_LIMIT)
        .map_err(|e| CiError::new(format!("{what}: {e}")))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn is_failure(status: StatusCode) -> bool {
    status.is_client_error() || status.is_server_error()
}

fn status_error(what: &str, response: &mut Response<ureq::Body>) -> CiError {
    let status = response.status().as_u16();
    let mut raw = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(ERROR_BODY_LIMIT)
        .read_to_end(&mut raw)
        .ok();
    let mut details = describe_body(&raw);
    for name in RESPONSE_HEADERS {
        if let Some(value) = response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
        {
            details.push(format!("{name}: {}", clean(value)));
        }
    }
    let suffix = if details.is_empty() {
        String::new()
    } else {
        format!(" ({})", details.join(", "))
    };
    let message = format!("{what}: http status: {status}{suffix}");
    if matches!(status, 404 | 410) {
        CiError::missing(message)
    } else {
        CiError::new(message)
    }
}

fn describe_body(raw: &[u8]) -> Vec<String> {
    if let Ok(serde_json::Value::Object(fields)) = serde_json::from_slice(raw) {
        let details: Vec<String> = ["message", "documentation_url"]
            .into_iter()
            .filter_map(|key| {
                fields
                    .get(key)
                    .and_then(serde_json::Value::as_str)
                    .map(|text| format!("{key}: {}", clean(text)))
            })
            .collect();
        if !details.is_empty() {
            return details;
        }
    }
    let text = clean(&String::from_utf8_lossy(raw));
    if text.is_empty() {
        Vec::new()
    } else {
        vec![format!("body: {text}")]
    }
}

fn clean(text: &str) -> String {
    let printable: String = text.chars().filter(|c| !c.is_control()).collect();
    crate::escape::inert(clip(&printable))
}

fn clip(text: &str) -> &str {
    let mut end = text.len().min(DETAIL_LIMIT);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

impl GitHub for HttpGitHub {
    fn get(&self, path: &str) -> Result<String, CiError> {
        let url = format!("{}{path}", self.api);
        body(
            &format!("GET {path}"),
            self.signed(self.agent.get(&url)).call(),
        )
    }
}

impl GitHubPost for HttpGitHub {
    fn post(&self, path: &str, json: &str) -> Result<String, CiError> {
        let url = format!("{}{path}", self.api);
        body(
            &format!("POST {path}"),
            self.signed(self.agent.post(&url))
                .header("Content-Type", "application/json")
                .send(json),
        )
    }
}

impl GitHubWrite for HttpGitHub {
    fn wait(&self, duration: Duration) {
        std::thread::sleep(duration);
    }

    fn upload(&self, url: &str, bytes: &[u8]) -> Result<String, CiError> {
        body(
            &format!("POST {url}"),
            self.signed(self.agent.post(url))
                .header("Content-Type", "application/octet-stream")
                .send(bytes),
        )
    }

    fn patch(&self, path: &str, json: &str) -> Result<String, CiError> {
        let url = format!("{}{path}", self.api);
        body(
            &format!("PATCH {path}"),
            self.signed(self.agent.patch(&url))
                .header("Content-Type", "application/json")
                .send(json),
        )
    }

    fn delete(&self, path: &str) -> Result<(), CiError> {
        let url = format!("{}{path}", self.api);
        body(
            &format!("DELETE {path}"),
            self.signed(self.agent.delete(&url)).call(),
        )
        .map(drop)
    }
}

impl HttpGitHub {
    /// Open the content of a release asset. The API answers with a redirect to signed storage,
    /// and the agent drops the `Authorization` header when it follows it.
    pub(super) fn open_asset(&self, path: &str) -> Result<Box<dyn Read + Send>, CiError> {
        let url = format!("{}{path}", self.api);
        let what = format!("GET {path}");
        let mut response = self
            .agent
            .get(&url)
            .config()
            .redirect_auth_headers(ureq::config::RedirectAuthHeaders::Never)
            .build()
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/octet-stream")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "velnor-ci")
            .call()
            .map_err(|e| CiError::new(format!("{what}: {e}")))?;
        if is_failure(response.status()) {
            return Err(status_error(&what, &mut response));
        }
        Ok(Box::new(response.into_body().into_reader()))
    }

    /// The body of a GET of `path`, `None` on any failure or a body over `limit` bytes.
    pub(super) fn get_bytes(&self, path: &str, limit: u64) -> Option<Vec<u8>> {
        let url = format!("{}{path}", self.api);
        let mut response = self.signed(self.agent.get(&url)).call().ok()?;
        if is_failure(response.status()) {
            return None;
        }
        super::read_limited(response.body_mut().as_reader(), limit).ok()
    }
}
