// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

use serde::Deserialize;
use std::fmt;

const BROKER_ERROR_SOURCE: &str = "actions-broker-listener";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrokerErrorCategory {
    Transient,
    Terminal,
    Conflict,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceErrorKind {
    RunnerNotFound,
    RunnerVersionTooOld,
    HostedRunnerDeprovisioned,
    RunnerSessionInvalid,
    AcknowledgeJobNotFound,
    JobNotFound,
}

impl ServiceErrorKind {
    fn from_broker_name(name: &str) -> Option<Self> {
        match name {
            "RunnerNotFound" => Some(Self::RunnerNotFound),
            "RunnerVersionTooOld" => Some(Self::RunnerVersionTooOld),
            "HostedRunnerDeprovisioned" => Some(Self::HostedRunnerDeprovisioned),
            "RunnerSessionInvalid" => Some(Self::RunnerSessionInvalid),
            "AcknowledgeJobNotFound" => Some(Self::AcknowledgeJobNotFound),
            _ => None,
        }
    }
}

#[derive(Deserialize)]
struct BrokerError {
    source: Option<String>,
    #[serde(rename = "errorKind")]
    error_kind: Option<String>,
}

pub(crate) fn broker_error_kind(body: &str) -> Option<ServiceErrorKind> {
    let error: BrokerError = serde_json::from_str(body).ok()?;
    if error.source.as_deref() != Some(BROKER_ERROR_SOURCE) {
        return None;
    }
    ServiceErrorKind::from_broker_name(error.error_kind.as_deref()?)
}

#[derive(thiserror::Error)]
#[error("{action} failed: status={status}, body={body}")]
pub struct GitHubApiError {
    pub status: u16,
    pub action: String,
    pub body: String,
    pub category: Option<BrokerErrorCategory>,
    pub kind: Option<ServiceErrorKind>,
}

impl fmt::Debug for GitHubApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GitHubApiError")
            .field("status", &self.status)
            .field("action", &self.action)
            .field("body", &"<redacted>")
            .field("category", &self.category)
            .field("kind", &self.kind)
            .finish()
    }
}

#[must_use]
pub fn broker_error_category(error: &anyhow::Error) -> Option<BrokerErrorCategory> {
    error.chain().find_map(|cause| {
        cause
            .downcast_ref::<GitHubApiError>()
            .and_then(|api| api.category)
    })
}

#[must_use]
pub fn service_error_kind(error: &anyhow::Error) -> Option<ServiceErrorKind> {
    error.chain().find_map(|cause| {
        cause
            .downcast_ref::<GitHubApiError>()
            .and_then(|api| api.kind)
    })
}

pub(crate) fn github_api_error(
    action: impl Into<String>,
    status: u16,
    body: &str,
) -> anyhow::Error {
    github_api_error_with(action, status, body, None, None)
}

pub(crate) fn github_api_error_categorized(
    action: impl Into<String>,
    status: u16,
    body: &str,
    category: BrokerErrorCategory,
) -> anyhow::Error {
    github_api_error_with(action, status, body, Some(category), None)
}

pub(crate) fn github_api_error_with(
    action: impl Into<String>,
    status: u16,
    body: &str,
    category: Option<BrokerErrorCategory>,
    kind: Option<ServiceErrorKind>,
) -> anyhow::Error {
    GitHubApiError {
        status,
        action: action.into(),
        body: sanitize_response_body(body),
        category,
        kind,
    }
    .into()
}

pub(crate) fn sanitize_response_body(raw: &str) -> String {
    const MAX_RESPONSE_BODY_BYTES: usize = 4096;
    const SENSITIVE_MARKERS: &[&str] = &[
        "authorization:",
        "bearer ",
        "access_token",
        "access-token",
        "client_secret",
        "client-secret",
        "password",
        "private_key",
        "private-key",
        "secret",
        "signature=",
        "sig=",
        "token",
        "ghp_",
        "gho_",
        "ghs_",
        "github_pat_",
        "begin private key",
    ];
    let lower = raw.to_ascii_lowercase();
    if SENSITIVE_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
    {
        return "<redacted response body>".to_owned();
    }

    let mut output = String::with_capacity(raw.len().min(MAX_RESPONSE_BODY_BYTES));
    for character in raw.chars() {
        let character = if character.is_control() || character.is_whitespace() {
            ' '
        } else {
            character
        };
        if output.len() + character.len_utf8() > MAX_RESPONSE_BODY_BYTES {
            output.push('…');
            break;
        }
        output.push(character);
    }
    output
}

#[must_use]
pub(crate) fn is_retriable_status(status: u16) -> bool {
    !(400..500).contains(&status) || matches!(status, 408 | 409 | 429)
}
