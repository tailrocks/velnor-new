// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use serde::Deserialize;
use velnor_model::GitRepository;

const TIMEOUT: Duration = Duration::from_secs(15);
const BODY_LIMIT: u64 = 2 * 1024 * 1024;

#[derive(Deserialize)]
struct Lookup {
    private: bool,
    full_name: String,
}

/// Asks the GitHub REST API, with no credentials, whether `repository` is public.
///
/// Every failure is an answer of "not proven public": a missing repository, a private one, a
/// redirect, a rate limit, a timeout and an unreadable reply.
pub(super) fn check(api: &str, repository: &GitRepository) -> Result<(), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .max_redirects(0)
        .max_redirects_will_error(false)
        .build()
        .into();
    let url = format!(
        "{}/repos/{}/{}",
        api.trim_end_matches('/'),
        repository.owner,
        repository.name
    );
    let mut response = agent
        .get(&url)
        .header("User-Agent", "velnor-ci")
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .call()
        .map_err(|error| format!("lookup failed: {error}"))?;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(match status {
            404 => {
                "HTTP 404 without a token: the repository is private or does not exist".to_owned()
            }
            403 | 429 => format!("HTTP {status}: rate limited or refused"),
            300..=399 => {
                format!("HTTP {status}: the repository moved, so the lock names another repository")
            }
            _ => format!("HTTP {status}"),
        });
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
    Ok(())
}
