// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;
use std::time::Duration;

use serde::Deserialize;
use velnor_model::GitRepository;
use velnor_rust::PinnedGit;

use crate::public_probe::{Deadline, bearer_allowed};

const BODY_LIMIT: u64 = 2 * 1024 * 1024;
const TOKEN_VARIABLE: &str = "GITHUB_TOKEN";

#[derive(Deserialize)]
struct Lookup {
    private: bool,
    full_name: String,
}

#[derive(Deserialize)]
struct Tree {
    tree: Vec<TreeEntry>,
    truncated: bool,
}

#[derive(Deserialize)]
struct TreeEntry {
    path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Rejection {
    Private(String),
    Unproven(String),
}

impl Rejection {
    pub(super) fn is_private(&self) -> bool {
        matches!(self, Self::Private(_))
    }
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Private(reason) | Self::Unproven(reason) => f.write_str(reason),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Credentials {
    Token,
    None,
}

pub(super) fn env_token() -> Option<String> {
    std::env::var(TOKEN_VARIABLE)
        .ok()
        .map(|token| token.trim().to_owned())
        .filter(|token| !token.is_empty())
}

pub(super) fn verify(
    api: &str,
    pinned: &PinnedGit,
    token: Option<&str>,
    deadline: Deadline,
) -> Result<Credentials, Rejection> {
    let credentials = check(api, &pinned.repository, token, deadline)?;
    for commit in &pinned.commits {
        check_submodules(api, &pinned.repository, commit, token, deadline)?;
    }
    Ok(credentials)
}

pub(super) fn check(
    api: &str,
    repository: &GitRepository,
    token: Option<&str>,
    deadline: Deadline,
) -> Result<Credentials, Rejection> {
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
    read(&url, repository, token, credentials, deadline)
}

fn read(
    url: &str,
    repository: &GitRepository,
    token: Option<&str>,
    credentials: Credentials,
    deadline: Deadline,
) -> Result<Credentials, Rejection> {
    let mut response = get(url, token, deadline).map_err(Rejection::Unproven)?;
    let status = response.status().as_u16();
    if status != 200 {
        let reason = refusal(status, credentials);
        return Err(if status == 404 {
            Rejection::Private(reason)
        } else {
            Rejection::Unproven(reason)
        });
    }
    let bytes = super::read_limited(response.body_mut().as_reader(), BODY_LIMIT)
        .map_err(|error| Rejection::Unproven(error.to_string()))?;
    let lookup: Lookup = serde_json::from_slice(&bytes)
        .map_err(|error| Rejection::Unproven(format!("unreadable reply: {error}")))?;
    let full = format!("{}/{}", repository.owner, repository.name);
    if !lookup.full_name.eq_ignore_ascii_case(&full) {
        return Err(Rejection::Unproven(format!(
            "the reply is for {}",
            lookup.full_name
        )));
    }
    if lookup.private {
        return Err(Rejection::Private("the repository is private".to_owned()));
    }
    Ok(credentials)
}

fn check_submodules(
    api: &str,
    repository: &GitRepository,
    commit: &str,
    token: Option<&str>,
    deadline: Deadline,
) -> Result<(), Rejection> {
    let url = format!(
        "{}/repos/{}/{}/git/trees/{commit}",
        api.trim_end_matches('/'),
        repository.owner,
        repository.name
    );
    let token = token.filter(|_| bearer_allowed(&url));
    let unproven = |reason: String| Rejection::Unproven(format!("commit {commit}: {reason}"));
    let mut response = get(&url, token, deadline).map_err(unproven)?;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(unproven(format!("HTTP {status} reading the file tree")));
    }
    let bytes = super::read_limited(response.body_mut().as_reader(), BODY_LIMIT)
        .map_err(|error| unproven(error.to_string()))?;
    let tree: Tree = serde_json::from_slice(&bytes)
        .map_err(|error| unproven(format!("unreadable file tree: {error}")))?;
    if tree.truncated {
        return Err(unproven("the file tree is too large to read".to_owned()));
    }
    if tree.tree.iter().any(|entry| entry.path == ".gitmodules") {
        return Err(unproven(
            "the repository has git submodules, whose repositories are not proven public"
                .to_owned(),
        ));
    }
    Ok(())
}

fn get(
    url: &str,
    token: Option<&str>,
    deadline: Deadline,
) -> Result<ureq::http::Response<ureq::Body>, String> {
    let timeout: Duration = deadline
        .request_timeout()
        .ok_or_else(|| "the time budget for the lookups ran out".to_owned())?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
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
    request
        .call()
        .map_err(|error| redact(format!("lookup failed: {error}"), token))
}

fn redact(text: String, token: Option<&str>) -> String {
    match token {
        Some(token) => text.replace(token, "***"),
        None => text,
    }
}

fn refusal(status: u16, credentials: Credentials) -> String {
    let access = match credentials {
        Credentials::Token => "with the job token",
        Credentials::None => "without a token",
    };
    match status {
        404 => format!("HTTP 404 {access}: the repository is private or does not exist"),
        403 | 429 => format!("HTTP {status}: rate limited or refused"),
        300..=399 => {
            format!("HTTP {status}: the repository moved, so the lock names another repository")
        }
        _ => format!("HTTP {status}"),
    }
}

#[cfg(test)]
mod tests;
