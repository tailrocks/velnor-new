// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

use crate::error::CiError;
use crate::github::GitHub;

const QUEUE_PREFIX: &str = "gh-readonly-queue/";
const REFS_HEADS: &str = "refs/heads/";
const SHA_LENGTH: usize = 40;
const REFS_PAGE: usize = 100;

#[derive(Debug, PartialEq, Eq)]
pub struct QueueEntry {
    pub base: String,
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

#[derive(Deserialize)]
struct QueueRef {
    r#ref: String,
    object: RefObject,
}

#[derive(Deserialize)]
struct RefObject {
    sha: String,
}

#[derive(Deserialize)]
struct Comparison {
    status: String,
}

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
    if base.is_empty() || !digits || !is_sha(sha) {
        return Err(malformed());
    }
    Ok(QueueEntry {
        base: base.to_owned(),
        number: number.parse().map_err(|_| malformed())?,
        sha: sha.to_ascii_lowercase(),
    })
}

fn is_sha(text: &str) -> bool {
    text.len() == SHA_LENGTH && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn check(head_ref: &str, repository: &str, github: &dyn GitHub) -> Result<(), CiError> {
    let entry = parse_queue_ref(head_ref)?;
    authenticate(&entry, repository, github)?;
    for earlier in entries_in_checkout(&entry, repository, github)? {
        authenticate(&earlier, repository, github)?;
    }
    Ok(())
}

fn authenticate(entry: &QueueEntry, repository: &str, github: &dyn GitHub) -> Result<(), CiError> {
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

fn entries_in_checkout(
    entry: &QueueEntry,
    repository: &str,
    github: &dyn GitHub,
) -> Result<Vec<QueueEntry>, CiError> {
    let path = format!(
        "/repos/{repository}/git/matching-refs/heads/{QUEUE_PREFIX}{}/?per_page={REFS_PAGE}",
        entry.base
    );
    let text = github.get(&path)?;
    let refs: Vec<QueueRef> = serde_json::from_str(&text)
        .map_err(|e| CiError::new(format!("GET {path} is not readable: {e}")))?;
    if refs.len() >= REFS_PAGE {
        return Err(CiError::new(format!(
            "the merge queue for {} has {} or more entries, too many to authenticate",
            entry.base, REFS_PAGE
        )));
    }
    let mut ours = None;
    let mut others = Vec::new();
    for queued in refs {
        let parsed = parse_queue_ref(&queued.r#ref)?;
        if parsed.base != entry.base {
            continue;
        }
        if !is_sha(&queued.object.sha) {
            return Err(CiError::new(format!(
                "queue ref {} points at `{}`, not a commit",
                queued.r#ref, queued.object.sha
            )));
        }
        if parsed.number == entry.number && parsed.sha == entry.sha {
            ours = Some(queued.object.sha);
        } else {
            others.push((parsed, queued.object.sha));
        }
    }
    let checkout = ours.ok_or_else(|| {
        CiError::new(format!(
            "the merge queue does not list pr-{}-{}",
            entry.number, entry.sha
        ))
    })?;
    let mut earlier = Vec::new();
    for (other, group) in others {
        let path = format!("/repos/{repository}/compare/{group}...{checkout}?per_page=1");
        let text = github.get(&path)?;
        let comparison: Comparison = serde_json::from_str(&text)
            .map_err(|e| CiError::new(format!("GET {path} is not readable: {e}")))?;
        match comparison.status.as_str() {
            "ahead" | "identical" => earlier.push(other),
            "behind" | "diverged" => {}
            status => {
                return Err(CiError::new(format!(
                    "GET {path} answered the unknown status `{status}`"
                )));
            }
        }
    }
    Ok(earlier)
}
