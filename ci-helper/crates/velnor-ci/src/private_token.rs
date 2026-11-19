// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

use crate::error::CiError;
use crate::github::GitHub;

const QUEUE_PREFIX: &str = "gh-readonly-queue/";
const REFS_HEADS: &str = "refs/heads/";
const SHA_LENGTH: usize = 40;

#[derive(Debug, PartialEq, Eq)]
pub struct QueueEntry {
    pub number: u64,
    pub sha: String,
}

#[derive(Deserialize)]
struct Pull {
    head: Head,
}

#[derive(Deserialize)]
struct Head {
    sha: String,
    repo: Option<HeadRepo>,
}

#[derive(Deserialize)]
struct HeadRepo {
    full_name: String,
}

/// Parses `refs/heads/gh-readonly-queue/<base>/pr-<N>-<sha>`, anchored on the last path segment
/// because the base branch may contain `/`.
pub fn parse_queue_ref(head_ref: &str) -> Result<QueueEntry, CiError> {
    let malformed = || {
        CiError::new(format!(
            "merge_group.head_ref `{head_ref}` is not gh-readonly-queue/<base>/pr-<number>-<sha>"
        ))
    };
    let name = head_ref.strip_prefix(REFS_HEADS).unwrap_or(head_ref);
    let (path, last) = name.rsplit_once('/').ok_or_else(malformed)?;
    let base = path.strip_prefix(QUEUE_PREFIX).ok_or_else(malformed)?;
    let (number, sha) = last
        .strip_prefix("pr-")
        .and_then(|rest| rest.split_once('-'))
        .ok_or_else(malformed)?;
    let digits = !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit());
    let hex = sha.len() == SHA_LENGTH && sha.bytes().all(|byte| byte.is_ascii_hexdigit());
    if base.is_empty() || !digits || !hex {
        return Err(malformed());
    }
    Ok(QueueEntry {
        number: number.parse().map_err(|_| malformed())?,
        sha: sha.to_ascii_lowercase(),
    })
}

/// Allows the private-source token only when the queued pull request's head is a commit of this
/// repository and is the commit the queue entry names. Every other answer is an error.
pub fn check(head_ref: &str, repository: &str, github: &dyn GitHub) -> Result<(), CiError> {
    let entry = parse_queue_ref(head_ref)?;
    let path = format!("/repos/{repository}/pulls/{}", entry.number);
    let text = github.get(&path)?;
    let pull: Pull = serde_json::from_str(&text)
        .map_err(|e| CiError::new(format!("GET {path} is not readable: {e}")))?;
    let full_name = pull.head.repo.as_ref().map(|repo| repo.full_name.as_str());
    if full_name != Some(repository) {
        return Err(CiError::new(format!(
            "pull request #{} comes from {}, not from {repository}",
            entry.number,
            full_name.unwrap_or("a deleted repository")
        )));
    }
    if !pull.head.sha.eq_ignore_ascii_case(&entry.sha) {
        return Err(CiError::new(format!(
            "pull request #{} is at {}, but the queue entry is {}",
            entry.number, pull.head.sha, entry.sha
        )));
    }
    Ok(())
}
