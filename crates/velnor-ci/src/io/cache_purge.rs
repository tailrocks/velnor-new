// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use super::cache_api::{Entry, delete_entries, pages};
use crate::cache::encode;
use crate::cli_cache::PurgePrivateArgs;
use crate::error::CiError;
use crate::escape::inert;
use crate::github::{GitHubWrite, Repo};
use crate::private_repo::require_private;

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
        .filter(|workflow| workflow.state == "active")
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
    let pending = private_entries(github, repo)?;
    let report = entry_report(&pending, repo);
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
    sweep(github, repo, pending, &report)
}

fn entry_report(entries: &[Entry], repo: &Repo) -> String {
    let bytes: u128 = entries
        .iter()
        .map(|entry| u128::from(entry.size_in_bytes))
        .sum();
    let refs: BTreeSet<_> = entries.iter().map(|entry| &entry.reference).collect();
    format!(
        "{} entries ({bytes} bytes) in {} refs of {repo} have a key starting with {PRIVATE_PREFIX}\n",
        entries.len(),
        refs.len()
    )
}

fn sweep(
    github: &dyn GitHubWrite,
    repo: &Repo,
    mut pending: Vec<Entry>,
    report: &str,
) -> Result<String, CiError> {
    if pending.is_empty() {
        return Ok(format!("{report}deleted 0 entries\n"));
    }
    let mut deleted = 0;
    let mut seen = 0;
    for _ in 0..PURGE_PASSES {
        seen += pending.len();
        let failures: Vec<String> = pending
            .iter()
            .filter_map(|entry| {
                match delete_entries(github, repo, &[entry.id]) {
                    Ok(()) => deleted += 1,
                    Err(error) => return Some(format!("ID {}: {error}", entry.id)),
                }
                None
            })
            .collect();
        if !failures.is_empty() {
            return Err(CiError::new(format!(
                "deleted {deleted} of {seen}; failed {} entries: {}",
                failures.len(),
                failures.join("; ")
            )));
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

pub fn purge_private_if_public(github: &dyn GitHubWrite, repo: &Repo) -> Result<String, CiError> {
    let lookup = match require_private(&repo.to_string(), github) {
        Ok(()) => {
            return Ok(format!(
                "{repo} is private; its {PRIVATE_PREFIX} entries are kept\n"
            ));
        }
        Err(error) => inert(&error.to_string()),
    };
    let pending = private_entries(github, repo)?;
    let report = format!("{lookup}\n{}", entry_report(&pending, repo));
    sweep(github, repo, pending, &report)
        .map_err(|error| CiError::new(format!("{lookup}: {error}")))
}

pub(super) fn run(args: &PurgePrivateArgs) -> Result<String, CiError> {
    let (github, repo) = super::cache_api::client(args.repo.as_deref())?;
    if args.if_public {
        purge_private_if_public(&github, &repo)
    } else {
        purge_private(&github, &repo, args.yes)
    }
}
