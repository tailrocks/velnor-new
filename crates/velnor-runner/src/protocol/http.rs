// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

use anyhow::{Context, Result, bail};
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, RETRY_AFTER, USER_AGENT};
use reqwest::{Client, Method};
use std::sync::OnceLock;
use std::time::Duration;
use url::Url;

use crate::secret::Secret;

pub const RUNNER_VERSION: &str = "2.338.2";
pub const RUNNER_USER_AGENT: &str = "actions-runner/2.338.2 (velnor)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerOs {
    Linux,
    MacOs,
    Windows,
}

impl RunnerOs {
    pub fn parse(os: &str) -> Result<Self> {
        match os {
            "linux" => Ok(Self::Linux),
            "macos" => Ok(Self::MacOs),
            "windows" => Ok(Self::Windows),
            other => bail!("unsupported runner operating system: {other}"),
        }
    }

    pub fn detect() -> Result<Self> {
        Self::parse(std::env::consts::OS)
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "Linux",
            Self::MacOs => "macOS",
            Self::Windows => "Windows",
        }
    }
}

pub(crate) fn runner_architecture() -> Result<&'static str> {
    match std::env::consts::ARCH {
        "x86" => Ok("X86"),
        "x86_64" => Ok("X64"),
        "arm" => Ok("ARM"),
        "aarch64" => Ok("ARM64"),
        other => bail!("unsupported runner architecture: {other}"),
    }
}

const ACTIONS_SERVICE_SUFFIX: &str = ".actions.githubusercontent.com";

#[derive(Debug)]
pub struct GithubHttpResponse {
    pub status: u16,
    pub body: Secret<String>,
    pub headers: Secret<HeaderMap>,
}

pub(crate) fn http_client() -> Result<Client> {
    static CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
    match CLIENT.get_or_init(|| {
        Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(RUNNER_USER_AGENT)
            .build()
            .map_err(|error| format!("build GitHub HTTP client: {error}"))
    }) {
        Ok(client) => Ok(client.clone()),
        Err(message) => bail!("{message}"),
    }
}

pub async fn github_json_http_response(
    method: Method,
    url: &str,
    bearer_token: &str,
    json_body: Option<String>,
    timeout: Duration,
) -> Result<GithubHttpResponse> {
    validate_authenticated_url(url)?;
    let safe_url = redacted_authenticated_url(url);
    let mut request = http_client()?
        .request(method.clone(), url)
        .bearer_auth(bearer_token)
        .header(ACCEPT, "application/json")
        .timeout(timeout);
    if let Some(body) = json_body {
        request = request.header(CONTENT_TYPE, "application/json").body(body);
    }
    let response = request.send().await.map_err(|error| {
        anyhow::anyhow!(
            "send {method} {safe_url}: {}",
            redacted_reqwest_error(&error)
        )
    })?;
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let body = response.text().await.map_err(|error| {
        anyhow::anyhow!(
            "read {method} {safe_url} response: {}",
            redacted_reqwest_error(&error)
        )
    })?;
    Ok(GithubHttpResponse {
        status,
        body: Secret::new(body),
        headers: Secret::new(headers),
    })
}

pub async fn github_json_request(
    method: Method,
    url: &str,
    bearer_token: &str,
    json_body: Option<String>,
    timeout: Duration,
) -> Result<(u16, String)> {
    let response = github_json_http_response(method, url, bearer_token, json_body, timeout).await?;
    Ok((response.status, response.body.into_inner()))
}

pub(crate) struct OAuthResponse {
    pub(crate) status: u16,
    pub(crate) body: String,
    pub(crate) json: bool,
    pub(crate) retry_after: Option<String>,
}

fn is_json_content(headers: &HeaderMap, body: &str) -> bool {
    !body.is_empty()
        && headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .is_some_and(|media| media.trim().eq_ignore_ascii_case("application/json"))
}

pub(crate) async fn oauth_form_request(
    url: &str,
    form_body: &str,
    timeout: Duration,
) -> Result<OAuthResponse> {
    validate_authenticated_url(url)?;
    let response = http_client()?
        .post(url)
        .header(USER_AGENT, RUNNER_USER_AGENT)
        .header(ACCEPT, "application/json")
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .timeout(timeout)
        .body(form_body.to_owned())
        .send()
        .await
        .map_err(|error| {
            anyhow::anyhow!(
                "send OAuth token request: {}",
                redacted_reqwest_error(&error)
            )
        })?;
    let status = response.status().as_u16();
    let retry_after = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let headers = response.headers().clone();
    let body = response.text().await.map_err(|error| {
        anyhow::anyhow!(
            "read OAuth token response: {}",
            redacted_reqwest_error(&error)
        )
    })?;
    Ok(OAuthResponse {
        status,
        json: is_json_content(&headers, &body),
        body,
        retry_after,
    })
}

pub(crate) fn validate_authenticated_url(raw: &str) -> Result<Url> {
    let safe = redacted_authenticated_url(raw);
    let url = Url::parse(raw).with_context(|| format!("parse authenticated URL '{safe}'"))?;
    if !url.username().is_empty() || url.password().is_some() {
        bail!("authenticated URL must not contain userinfo");
    }
    match url.scheme() {
        "https" => Ok(url),
        "http"
            if cfg!(feature = "test-support") && url.host_str().is_some_and(is_loopback_host) =>
        {
            Ok(url)
        }
        _ => bail!("authenticated endpoint must use HTTPS: {safe}"),
    }
}

pub(crate) fn is_github_actions_service_host(host: &str) -> bool {
    let Some(prefix_len) = host.len().checked_sub(ACTIONS_SERVICE_SUFFIX.len()) else {
        return false;
    };
    let (prefix, suffix) = host.split_at(prefix_len);
    if !suffix.eq_ignore_ascii_case(ACTIONS_SERVICE_SUFFIX) || prefix.is_empty() {
        return false;
    }
    let bytes = prefix.as_bytes();
    bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && prefix
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
}

pub(crate) fn validate_known_service_url(raw: &str, field: &str, hosts: &[&str]) -> Result<Url> {
    let url = validate_authenticated_url(raw)?;
    let host = url.host_str().unwrap_or_default();
    let loopback = cfg!(feature = "test-support") && is_loopback_host(host);
    let allowed_host = loopback
        || hosts.iter().any(|allowed| {
            host.eq_ignore_ascii_case(allowed)
                || (is_github_actions_service_host(host) && is_github_actions_service_host(allowed))
        });
    if !allowed_host || !(loopback || url.port().is_none()) {
        bail!(
            "{field} endpoint host is not an approved GitHub Actions service: {}",
            redacted_authenticated_url(raw)
        );
    }
    if url.query().is_some() || url.fragment().is_some() {
        bail!("{field} endpoint must not contain query or fragment data");
    }
    Ok(url)
}

pub(crate) fn slash_url(url: &Url) -> Url {
    let mut root = url.clone();
    if !root.path().ends_with('/') {
        let path = format!("{}/", root.path());
        root.set_path(&path);
    }
    root
}

#[must_use]
pub fn redacted_authenticated_url(raw: &str) -> String {
    let Ok(mut url) = Url::parse(raw) else {
        return "<invalid URL>".to_owned();
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

pub(crate) fn redacted_reqwest_error(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "request timed out"
    } else if error.is_redirect() {
        "request redirect failed"
    } else if error.is_body() {
        "request body transfer failed"
    } else if error.is_decode() {
        "response decoding failed"
    } else if error.is_builder() {
        "request construction failed"
    } else {
        "request transport failed"
    }
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || matches!(host, "127.0.0.1" | "[::1]" | "::1")
}
