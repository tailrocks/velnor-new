// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use velnor_rust::{CacheEntry, budget_plan, cache_family, delta_pull_request};

use crate::cache::stale_entries;
use crate::cli::BudgetArgs;
use crate::error::CiError;
use crate::github::{GitHubWrite, Repo};
use crate::time::parse_utc;

const PAGE: usize = 100;

#[derive(Deserialize)]
struct Listing<T> {
    total_count: usize,
    actions_caches: Vec<T>,
}

#[derive(Deserialize)]
struct Entry {
    id: u64,
    key: String,
    #[serde(rename = "ref")]
    reference: String,
    size_in_bytes: u64,
    created_at: String,
    last_accessed_at: String,
}

#[derive(Deserialize)]
struct PullRequest {
    number: u64,
}

fn pages<T: serde::de::DeserializeOwned>(
    github: &dyn GitHubWrite,
    path: &str,
) -> Result<Vec<T>, CiError> {
    let mut entries = Vec::new();
    let mut page = 1;
    let mut expected_total = None;
    loop {
        let json = github.get(&format!("{path}&per_page={PAGE}&page={page}"))?;
        let listing: Listing<T> = serde_json::from_str(&json)
            .map_err(|e| CiError::new(format!("the cache listing is not readable: {e}")))?;
        if expected_total
            .replace(listing.total_count)
            .is_some_and(|total| total != listing.total_count)
        {
            return Err(CiError::new("the cache listing changed during pagination"));
        }
        let count = listing.actions_caches.len();
        entries.extend(listing.actions_caches);
        if entries.len() >= listing.total_count {
            return Ok(entries);
        }
        if count < PAGE {
            return Err(CiError::new("the cache listing is incomplete"));
        }
        page += 1;
    }
}

fn delete_entries(github: &dyn GitHubWrite, repo: &Repo, ids: &[u64]) -> Result<(), CiError> {
    for id in ids {
        github.delete(&format!("/repos/{repo}/actions/caches/{id}"))?;
    }
    Ok(())
}

pub fn trim(
    github: &dyn GitHubWrite,
    repo: &Repo,
    reference: &str,
    prefix: &str,
    keep: usize,
) -> Result<String, CiError> {
    if prefix.is_empty() {
        return Err(CiError::new("the cache prefix is empty"));
    }
    let path = format!(
        "/repos/{repo}/actions/caches?key={}&ref={}",
        encode(prefix),
        encode(reference)
    );
    let entries: Vec<crate::cache::CacheEntry> = pages(github, &path)?;
    unique_ids(entries.iter().map(|entry| entry.id))?;
    let stale = stale_entries(&entries, prefix, keep);
    delete_entries(github, repo, &stale)?;
    Ok(format!(
        "{prefix}: {} entries on {reference}, kept the newest {keep}, deleted {}\n",
        entries
            .iter()
            .filter(|entry| entry.key.starts_with(prefix))
            .count(),
        stale.len()
    ))
}

fn timestamp(text: &str) -> Result<i64, CiError> {
    parse_utc(text).ok_or_else(|| CiError::new(format!("invalid cache timestamp: {text}")))
}

fn caches(github: &dyn GitHubWrite, repo: &Repo) -> Result<Vec<CacheEntry>, CiError> {
    pages::<Entry>(github, &format!("/repos/{repo}/actions/caches?"))?
        .into_iter()
        .map(|entry| {
            Ok(CacheEntry {
                id: entry.id,
                key: entry.key,
                reference: entry.reference,
                size_in_bytes: entry.size_in_bytes,
                created_at: timestamp(&entry.created_at)?,
                last_accessed_at: timestamp(&entry.last_accessed_at)?,
            })
        })
        .collect()
}

fn open_prs(github: &dyn GitHubWrite, repo: &Repo) -> Result<BTreeSet<u64>, CiError> {
    let mut prs = BTreeSet::new();
    let mut page = 1;
    loop {
        let json = github.get(&format!(
            "/repos/{repo}/pulls?state=open&per_page={PAGE}&page={page}"
        ))?;
        let entries: Vec<PullRequest> = serde_json::from_str(&json)
            .map_err(|e| CiError::new(format!("the pull request listing is not readable: {e}")))?;
        let count = entries.len();
        prs.extend(entries.into_iter().map(|pr| pr.number));
        if count < PAGE {
            return Ok(prs);
        }
        page += 1;
    }
}

pub fn budget(
    github: &dyn GitHubWrite,
    repo: &Repo,
    now: i64,
    budget_bytes: u64,
    dry_run: bool,
) -> Result<String, CiError> {
    let entries = caches(github, repo)?;
    unique_ids(entries.iter().map(|entry| entry.id))?;
    let mut prs = open_prs(github, repo)?;
    confirm_absent(github, repo, &entries, &mut prs)?;
    let ids = budget_plan(&entries, &prs, now, budget_bytes);
    let mut families = BTreeMap::new();
    for entry in &entries {
        let row = families.entry(cache_family(&entry.key)).or_insert((
            0_usize,
            0_u128,
            i64::MIN,
            i64::MAX,
        ));
        row.0 += 1;
        row.1 += u128::from(entry.size_in_bytes);
        row.2 = row.2.max(entry.created_at);
        row.3 = row.3.min(entry.created_at);
    }
    let before: u128 = entries
        .iter()
        .map(|entry| u128::from(entry.size_in_bytes))
        .sum();
    let selected: BTreeSet<_> = ids.iter().copied().collect();
    let removed: u128 = entries
        .iter()
        .filter(|entry| selected.contains(&entry.id))
        .map(|entry| u128::from(entry.size_in_bytes))
        .sum();
    let after = before - removed;
    let mut report = vec!["family | entries | bytes | newest (unix) | oldest (unix)\n".to_owned()];
    for (family, (count, bytes, newest, oldest)) in families {
        report.push(format!(
            "{family:?} | {count} | {bytes} | {newest} | {oldest}\n"
        ));
    }
    report.push(format!("before {before} bytes; selected {removed} bytes; projected after {after} bytes\nselected IDs: {ids:?}\n"));
    if after > u128::from(budget_bytes) {
        report.push(
            "::warning::retained caches exceed the budget; protected and unknown entries remain\n"
                .to_owned(),
        );
    }
    if !dry_run {
        delete_entries(github, repo, &ids)?;
    }
    report.push(format!(
        "{} {} entries; budget {budget_bytes} bytes\n",
        if dry_run { "would delete" } else { "deleted" },
        ids.len()
    ));
    Ok(report.concat())
}

pub(super) fn run_budget(args: &BudgetArgs) -> Result<String, CiError> {
    let repository = args
        .repo
        .clone()
        .or_else(|| std::env::var("GITHUB_REPOSITORY").ok())
        .ok_or_else(|| CiError::new("set GITHUB_REPOSITORY or pass --repo owner/name"))?;
    let github = super::http::HttpGitHub::new(
        super::api_url(),
        super::token()?,
        super::http::DEFAULT_TIMEOUT,
    );
    budget(
        &github,
        &Repo::parse(&repository)?,
        super::now_seconds(),
        args.budget_bytes,
        args.dry_run,
    )
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                char::from(byte).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

fn unique_ids(ids: impl Iterator<Item = u64>) -> Result<(), CiError> {
    let mut seen = BTreeSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(CiError::new("the cache listing repeats a cache ID"));
        }
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum PullState {
    Open,
    Closed,
}

#[derive(Deserialize)]
struct PullStatus {
    state: PullState,
}

fn confirm_absent(
    github: &dyn GitHubWrite,
    repo: &Repo,
    entries: &[CacheEntry],
    prs: &mut BTreeSet<u64>,
) -> Result<(), CiError> {
    let candidates: BTreeSet<_> = entries
        .iter()
        .filter_map(delta_pull_request)
        .filter(|number| !prs.contains(number))
        .collect();
    for number in candidates {
        let response = github.get(&format!("/repos/{repo}/pulls/{number}"))?;
        let status: PullStatus = serde_json::from_str(&response)
            .map_err(|e| CiError::new(format!("the pull request state is not readable: {e}")))?;
        if matches!(status.state, PullState::Open) {
            prs.insert(number);
        }
    }
    Ok(())
}
