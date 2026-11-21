// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use super::cache_api::{Entry, delete_entries, pages};
use crate::cache::encode;
use crate::cli_cache::PurgePrivateArgs;
use crate::error::CiError;
use crate::github::{GitHubWrite, Repo};

const PRIVATE_PREFIX: &str = "velnor-private-";
const PURGE_PASSES: usize = 3;

fn private_entries(github: &dyn GitHubWrite, repo: &Repo) -> Result<Vec<Entry>, CiError> {
    let path = format!(
        "/repos/{repo}/actions/caches?key={}",
        encode(PRIVATE_PREFIX)
    );
    let (listed, _) = pages::<Entry>(github, &path, None)?;
    let private: Vec<Entry> = listed
        .into_iter()
        .filter(|entry| entry.key.starts_with(PRIVATE_PREFIX))
        .collect();
    let ids: BTreeSet<u64> = private.iter().map(|entry| entry.id).collect();
    if ids.len() != private.len() {
        return Err(CiError::new("the cache listing repeats a cache ID"));
    }
    Ok(private)
}

#[derive(serde::Deserialize)]
struct RunCount {
    total_count: u64,
}

#[derive(serde::Deserialize)]
struct WorkflowList {
    total_count: u64,
    workflows: Vec<Workflow>,
}

#[derive(serde::Deserialize)]
struct Workflow {
    state: String,
}

fn enabled_workflows(github: &dyn GitHubWrite, repo: &Repo) -> Result<u64, CiError> {
    let json = github.get(&format!("/repos/{repo}/actions/workflows?per_page=100"))?;
    let list: WorkflowList = serde_json::from_str(&json)
        .map_err(|e| CiError::new(format!("the workflow listing is not readable: {e}")))?;
    if list.total_count != list.workflows.len() as u64 {
        return Err(CiError::new(
            "the workflow listing is incomplete; run purge-private again",
        ));
    }
    Ok(list
        .workflows
        .iter()
        .filter(|workflow| !workflow.state.starts_with("disabled"))
        .count() as u64)
}

fn running_workflows(github: &dyn GitHubWrite, repo: &Repo) -> Result<u64, CiError> {
    let mut total = 0;
    for status in ["in_progress", "queued", "waiting", "requested", "pending"] {
        let json = github.get(&format!(
            "/repos/{repo}/actions/runs?status={status}&per_page=1"
        ))?;
        let count: RunCount = serde_json::from_str(&json)
            .map_err(|e| CiError::new(format!("the workflow run listing is not readable: {e}")))?;
        total += count.total_count;
    }
    Ok(total)
}

pub fn purge_private(
    github: &dyn GitHubWrite,
    repo: &Repo,
    delete: bool,
) -> Result<String, CiError> {
    if delete {
        let enabled = enabled_workflows(github, repo)?;
        if enabled > 0 {
            return Err(CiError::new(format!(
                "{enabled} workflows are enabled; disable every workflow before the purge so no run can start during it"
            )));
        }
    }
    let first = running_workflows(github, repo)?;
    let second = running_workflows(github, repo)?;
    let running = first.max(second);
    let busy = format!(
        "{running} workflow runs are in progress; wait for them or disable workflows, then run purge-private again"
    );
    if delete && running > 0 {
        return Err(CiError::new(busy));
    }
    let mut pending = private_entries(github, repo)?;
    let bytes: u128 = pending
        .iter()
        .map(|entry| u128::from(entry.size_in_bytes))
        .sum();
    let refs: BTreeSet<_> = pending.iter().map(|entry| &entry.reference).collect();
    let report = format!(
        "{} entries ({bytes} bytes) in {} refs of {repo} have a key starting with {PRIVATE_PREFIX}\n",
        pending.len(),
        refs.len()
    );
    if !delete {
        let warning = if running > 0 {
            format!("warning: {busy}\n")
        } else {
            String::new()
        };
        return Ok(format!(
            "{report}would delete {} entries; pass --yes to delete them\n{warning}",
            pending.len()
        ));
    }
    let mut deleted = 0;
    let mut seen = 0;
    for _ in 0..PURGE_PASSES {
        let ids: Vec<u64> = pending.iter().map(|entry| entry.id).collect();
        seen += ids.len();
        for id in &ids {
            if let Err(error) = delete_entries(github, repo, &[*id]) {
                return Err(CiError::new(format!(
                    "deleted {deleted} of {seen}; failed at entry ID {id}: {error}"
                )));
            }
            deleted += 1;
        }
        pending = private_entries(github, repo)?;
        if pending.is_empty() {
            return Ok(format!("{report}deleted {deleted} entries\n"));
        }
    }
    Err(CiError::new(format!(
        "{} {PRIVATE_PREFIX} entries remain after the purge; run it again",
        pending.len()
    )))
}

pub(super) fn run(args: &PurgePrivateArgs) -> Result<String, CiError> {
    let (github, repo) = super::cache_api::client(args.repo.as_deref())?;
    purge_private(&github, &repo, args.yes)
}
