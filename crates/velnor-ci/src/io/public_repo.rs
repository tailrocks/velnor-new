// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use serde::Deserialize;
use velnor_model::GitRepository;

use crate::public_probe::bearer_allowed;

const TIMEOUT: Duration = Duration::from_secs(15);
const BODY_LIMIT: u64 = 2 * 1024 * 1024;
const TOKEN_VARIABLE: &str = "GITHUB_TOKEN";

#[derive(Deserialize)]
struct Lookup {
    private: bool,
    full_name: String,
}

/// How a lookup went, for the log: with the job's token or without.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Credentials {
    Token,
    None,
}

/// The job's own token, when the environment has one.
pub(super) fn env_token() -> Option<String> {
    std::env::var(TOKEN_VARIABLE)
        .ok()
        .map(|token| token.trim().to_owned())
        .filter(|token| !token.is_empty())
}

/// Asks the API whether `repository` is public. The token goes out only to the exact GitHub API
/// host over https (see `bearer_allowed`), and a redirect is never followed, so it cannot reach
/// another host.
pub(super) fn check(
    api: &str,
    repository: &GitRepository,
    token: Option<&str>,
) -> Result<Credentials, String> {
    let url = format!(
        "{}/repos/{}/{}",
        api.trim_end_matches('/'),
        repository.owner,
        repository.name
    );
    let token = token.filter(|_| bearer_allowed(&url));
    let credentials = if token.is_some() {
        Credentials::Token
    } else {
        Credentials::None
    };
    read(&url, repository, token, credentials)
}

fn read(
    url: &str,
    repository: &GitRepository,
    token: Option<&str>,
    credentials: Credentials,
) -> Result<Credentials, String> {
    let mut response = get(url, token)?;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(refusal(status, credentials));
    }
    let bytes = super::read_limited(response.body_mut().as_reader(), BODY_LIMIT)
        .map_err(|error| error.to_string())?;
    let lookup: Lookup =
        serde_json::from_slice(&bytes).map_err(|error| format!("unreadable reply: {error}"))?;
    let full = format!("{}/{}", repository.owner, repository.name);
    if !lookup.full_name.eq_ignore_ascii_case(&full) {
        return Err(format!("the reply is for {}", lookup.full_name));
    }
    if lookup.private {
        return Err("the repository is private".to_owned());
    }
    Ok(credentials)
}

fn get(url: &str, token: Option<&str>) -> Result<ureq::http::Response<ureq::Body>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .max_redirects(0)
        .max_redirects_will_error(false)
        .build()
        .into();
    let mut request = agent
        .get(url)
        .header("User-Agent", "velnor-ci")
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(token) = token {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    request.call().map_err(|error| {
        let text = format!("lookup failed: {error}");
        match token {
            Some(token) => text.replace(token, "***"),
            None => text,
        }
    })
}

fn refusal(status: u16, credentials: Credentials) -> String {
    let access = match credentials {
        Credentials::Token => "with the job token",
        Credentials::None => "without a token",
    };
    match status {
        404 => format!(
            "HTTP 404 {access}: the repository is private or does not exist"
        ),
        403 | 429 => format!("HTTP {status}: rate limited or refused"),
        300..=399 => {
            format!("HTTP {status}: the repository moved, so the lock names another repository")
        }
        _ => format!("HTTP {status}"),
    }
}

#[cfg(test)]
mod tests;
