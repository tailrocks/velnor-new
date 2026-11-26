// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrokerErrorCategory {
    Transient,
    Terminal,
    Conflict,
}

#[derive(thiserror::Error)]
#[error("{action} failed: status={status}, body={body}")]
pub struct GitHubApiError {
    pub status: u16,
    pub action: String,
    pub body: String,
    pub category: Option<BrokerErrorCategory>,
}

impl fmt::Debug for GitHubApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GitHubApiError")
            .field("status", &self.status)
            .field("action", &self.action)
            .field("body", &"<redacted>")
            .field("category", &self.category)
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

pub(crate) fn github_api_error(
    action: impl Into<String>,
    status: u16,
    body: &str,
) -> anyhow::Error {
    github_api_error_with(action, status, body, None)
}

pub(crate) fn github_api_error_categorized(
    action: impl Into<String>,
    status: u16,
    body: &str,
    category: BrokerErrorCategory,
) -> anyhow::Error {
    github_api_error_with(action, status, body, Some(category))
}

fn github_api_error_with(
    action: impl Into<String>,
    status: u16,
    body: &str,
    category: Option<BrokerErrorCategory>,
) -> anyhow::Error {
    GitHubApiError {
        status,
        action: action.into(),
        body: sanitize_response_body(body),
        category,
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
