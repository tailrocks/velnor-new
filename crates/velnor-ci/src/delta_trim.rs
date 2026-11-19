// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;
use crate::github::{GitHubWrite, Repo};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize)]
struct Identity {
    id: u64,
}
#[derive(Deserialize)]
struct Pull {
    number: u64,
}
#[derive(Deserialize)]
struct Run {
    id: u64,
    status: String,
    event: String,
    workflow_id: u64,
    repository: Identity,
    head_repository: Option<Identity>,
    head_sha: String,
    pull_requests: Vec<Pull>,
}
#[derive(Deserialize)]
struct Association {
    number: u64,
    base: Branch,
    head: Branch,
}
#[derive(Deserialize)]
struct Branch {
    repo: Option<Identity>,
}
#[derive(Deserialize)]
struct Side {
    repo: Identity,
}
#[derive(Deserialize)]
struct Request {
    base: Side,
    head: Side,
}
#[derive(Deserialize)]
struct Entry {
    id: u64,
    key: String,
    r#ref: String,
    created_at: String,
}
#[derive(Deserialize)]
struct Listing {
    total_count: usize,
    actions_caches: Vec<Entry>,
}

fn decode<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, CiError> {
    serde_json::from_str(text)
        .map_err(|e| CiError::new(format!("invalid delta cleanup response: {e}")))
}

pub fn trim_deltas(github: &dyn GitHubWrite, repo: &Repo, run_id: u64) -> Result<String, CiError> {
    let identity: Identity = decode(&github.get(&format!("/repos/{repo}"))?)?;
    let workflow: Identity =
        decode(&github.get(&format!("/repos/{repo}/actions/workflows/rust-ci.yml"))?)?;
    let run: Run = decode(&github.get(&format!("/repos/{repo}/actions/runs/{run_id}"))?)?;
    if run.id != run_id
        || run.status != "completed"
        || run.event != "pull_request"
        || run.workflow_id != workflow.id
        || run.repository.id != identity.id
        || run
            .head_repository
            .is_none_or(|head| head.id != identity.id)
        || run.pull_requests.len() > 1
    {
        return Err(CiError::new(
            "cleanup requires a completed, unambiguous same-repository Rust CI PR run",
        ));
    }
    let number = match run.pull_requests.as_slice() {
        [pull] => pull.number,
        _ => pull_from_commit(github, repo, &identity, &run.head_sha)?,
    };
    if number == 0 {
        return Err(CiError::new("cleanup PR number is zero"));
    }
    let request: Request = decode(&github.get(&format!("/repos/{repo}/pulls/{number}"))?)?;
    if request.base.repo.id != identity.id || request.head.repo.id != identity.id {
        return Err(CiError::new("cleanup PR is not from this repository"));
    }
    let reference = format!("refs/pull/{number}/merge");
    let prefix = format!("velnor-dlt-pr{number}-");
    let mut entries = Vec::new();
    let mut seen = BTreeSet::new();
    let mut total = None;
    let mut page = 1;
    loop {
        let listing: Listing = decode(&github.get(&format!("/repos/{repo}/actions/caches?key={prefix}&ref=refs%2Fpull%2F{number}%2Fmerge&per_page=100&page={page}"))?)?;
        let count = listing.actions_caches.len();
        if total.is_some_and(|total| total != listing.total_count)
            || listing
                .actions_caches
                .iter()
                .any(|entry| !seen.insert(entry.id))
        {
            return Err(CiError::new(
                "delta cleanup cache listing changed during pagination",
            ));
        }
        total = Some(listing.total_count);
        entries.extend(listing.actions_caches);
        if entries.len() == listing.total_count {
            break;
        }
        if count == 0 || entries.len() > listing.total_count {
            return Err(CiError::new("delta cleanup cache listing is incomplete"));
        }
        page += 1;
    }
    let mut lanes: BTreeMap<String, Vec<(i64, u64)>> = BTreeMap::new();
    for entry in entries {
        if entry.r#ref != reference {
            return Err(CiError::new(
                "delta cleanup cache listing contains another ref",
            ));
        }
        if let Some(owner) = owner(&entry.key, &prefix) {
            let time = crate::time::parse_utc(&entry.created_at)
                .ok_or_else(|| CiError::new("invalid delta creation time"))?;
            lanes.entry(owner).or_default().push((time, entry.id));
        }
    }
    let mut deleted = 0;
    for entries in lanes.values_mut() {
        entries.sort_unstable_by(|a, b| b.cmp(a));
        for (_, id) in entries.iter().skip(1) {
            github.delete(&format!("/repos/{repo}/actions/caches/{id}"))?;
            deleted += 1;
        }
    }
    Ok(format!(
        "PR {number}: kept one delta per lane, deleted {deleted} older entries\n"
    ))
}

// GitHub empties a run's pull_requests once its PR merges or closes while the run is
// still reported, so the PR is found through the run's head commit instead.
fn pull_from_commit(
    github: &dyn GitHubWrite,
    repo: &Repo,
    identity: &Identity,
    sha: &str,
) -> Result<u64, CiError> {
    if sha.len() != 40 || !sha.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(CiError::new("cleanup run has no valid head commit"));
    }
    let found: Vec<Association> =
        decode(&github.get(&format!("/repos/{repo}/commits/{sha}/pulls"))?)?;
    let same_repo: Vec<u64> = found
        .iter()
        .filter(|pull| {
            pull.base.repo.as_ref().is_some_and(|r| r.id == identity.id)
                && pull.head.repo.as_ref().is_some_and(|r| r.id == identity.id)
        })
        .map(|pull| pull.number)
        .collect();
    match same_repo.as_slice() {
        [number] if *number != 0 => Ok(*number),
        _ => Err(CiError::new(
            "cleanup run has no single same-repository PR for its commit",
        )),
    }
}

fn owner(key: &str, prefix: &str) -> Option<String> {
    let mut parts = key.strip_prefix(prefix)?.rsplitn(4, '-');
    parts.next()?.parse::<u64>().ok()?;
    let base = parts.next()?;
    let compat = parts.next()?;
    let owner = parts.next()?;
    if base.len() != 8
        || compat.len() != 16
        || !base
            .bytes()
            .chain(compat.bytes())
            .all(|c| c.is_ascii_hexdigit())
    {
        return None;
    }
    let segments: Vec<_> = owner.split('.').collect();
    if segments.len() != 3
        || segments.iter().any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
        })
    {
        return None;
    }
    Some(owner.to_owned())
}
