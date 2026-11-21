// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::cache_api::{Entry, client, delete_entries, exact_entries};
use crate::cache::encode;
use crate::cli_cache::DeleteEntryArgs;
use crate::error::CiError;
use crate::escape::{inert, shell_quote};
use crate::github::{GitHubWrite, Repo};
use crate::time::parse_utc;

const DEFAULT_API: &str = "https://api.github.com";

fn describe(entries: &[&Entry]) -> String {
    entries
        .iter()
        .map(|entry| {
            format!(
                "ID {} version {} on {}",
                entry.id,
                inert(&entry.version),
                inert(&entry.reference)
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn holders(
    github: &dyn GitHubWrite,
    repo: &Repo,
    key: &str,
    refs: &[String],
) -> Result<Vec<Entry>, CiError> {
    let mut held = Vec::new();
    for reference in refs {
        let path = format!(
            "/repos/{repo}/actions/caches?key={}&ref={}",
            encode(key),
            encode(reference)
        );
        held.extend(
            exact_entries(github, &path)?
                .into_iter()
                .filter(|entry| entry.key == key && entry.reference == *reference),
        );
    }
    Ok(held)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: u64,
    pub version: String,
    pub reference: String,
}

pub fn resolve_entry(
    github: &dyn GitHubWrite,
    repo: &Repo,
    key: &str,
    refs: &[String],
    restored_before: &str,
) -> Result<Candidate, CiError> {
    if key.is_empty() || refs.is_empty() || refs.iter().any(String::is_empty) {
        return Err(CiError::new("pass the exact cache key and its refs"));
    }
    let Some(restored) = parse_utc(restored_before) else {
        return Err(CiError::new(format!(
            "pass --restored-before as the RFC 3339 UTC time the notice printed, such as 2026-11-22T10:00:00Z; {} is not one; nothing was deleted",
            inert(restored_before)
        )));
    };
    let named = inert(key);
    let held = holders(github, repo, key, refs)?;
    if let Some(unversioned) = held.iter().find(|entry| entry.version.is_empty()) {
        return Err(CiError::new(format!(
            "the entry ID {} has the key {named} but the API reported no version for it, so the entries cannot be told apart; nothing was deleted",
            unversioned.id
        )));
    }
    let Some(first) = held.first() else {
        let searched: Vec<String> = refs.iter().map(|reference| inert(reference)).collect();
        return Err(CiError::new(format!(
            "no cache entry has the key {named} on {}; nothing was deleted",
            searched.join(" or ")
        )));
    };
    if first.reference != refs[0] {
        let earlier: Vec<String> = refs
            .iter()
            .take_while(|reference| **reference != first.reference)
            .map(|reference| inert(reference))
            .collect();
        let checked: Vec<String> = refs.iter().map(|reference| inert(reference)).collect();
        let holder = inert(&first.reference);
        return Err(CiError::new(format!(
            "the key {named} is no longer on {} (refs checked, in order: {}; the first ref that holds it is {holder}); if you or another job already deleted this run's entry, stop. Otherwise the restore read {holder}: re-run with `--ref {holder}` first only if you know this run restored from it; nothing was deleted",
            earlier.join(" or "),
            checked.join(", ")
        )));
    }
    let on_first: Vec<&Entry> = held
        .iter()
        .filter(|entry| entry.reference == first.reference)
        .collect();
    if on_first.len() > 1 {
        return Err(CiError::new(format!(
            "{} entries have the key {named} on {}; nothing was deleted. Entries found: {}",
            on_first.len(),
            inert(&first.reference),
            describe(&on_first)
        )));
    }
    let other_versions: Vec<&Entry> = held
        .iter()
        .filter(|entry| entry.version != first.version)
        .collect();
    if !other_versions.is_empty() {
        let all: Vec<&Entry> = held.iter().collect();
        return Err(CiError::new(format!(
            "the key {named} is held under more than one version, and the restore may have used any ref that holds it; nothing was deleted. Entries found: {}. Check which version the job used, then delete that entry with --id <ID> --key {}",
            describe(&all),
            shell_quote(key)
        )));
    }
    match parse_utc(&first.created_at) {
        Some(saved) if saved < restored => {}
        Some(_) => {
            return Err(CiError::new(format!(
                "the entry ID {} ({named}) was saved at {}, not before this run's restore ({}), so it is not the entry this run read: a later run saved it after the entry the restore read was deleted or evicted; nothing was deleted",
                first.id,
                inert(&first.created_at),
                inert(restored_before)
            )));
        }
        None => {
            return Err(CiError::new(format!(
                "the entry ID {} ({named}) has no usable created_at ({}), so it cannot be shown to predate this run's restore ({}); nothing was deleted",
                first.id,
                inert(&first.created_at),
                inert(restored_before)
            )));
        }
    }
    Ok(Candidate {
        id: first.id,
        version: first.version.clone(),
        reference: first.reference.clone(),
    })
}

pub fn delete_by_id(
    github: &dyn GitHubWrite,
    repo: &Repo,
    key: &str,
    id: u64,
) -> Result<String, CiError> {
    if key.is_empty() {
        return Err(CiError::new("pass the exact cache key and its refs"));
    }
    let named = inert(key);
    let path = format!("/repos/{repo}/actions/caches?key={}", encode(key));
    let entry = exact_entries(github, &path)?
        .into_iter()
        .find(|entry| entry.id == id && entry.key == key);
    let Some(entry) = entry else {
        return Err(CiError::new(format!(
            "no cache entry with ID {id} has the key {named}: it was deleted already or belongs to another key; nothing was deleted"
        )));
    };
    delete_entries(github, repo, &[entry.id])?;
    Ok(format!(
        "deleted cache entry ID {} ({named}, version {}, {})\n",
        entry.id,
        inert(&entry.version),
        inert(&entry.reference)
    ))
}

#[must_use]
pub fn resolved_report(repo: &Repo, key: &str, found: &Candidate, api_url: &str) -> String {
    let named = inert(key);
    let api_prefix = if api_url == DEFAULT_API {
        String::new()
    } else {
        format!("GITHUB_API_URL={} ", shell_quote(api_url))
    };
    format!(
        "cache entry ID {} ({named}, version {}, {}) is the entry on the first --ref, the ref the restore reads first; nothing was deleted\nThe two steps are needed once per run, not once per job: when another job of this run already deleted its entry, step 1 refuses instead of naming a later ref's entry. To delete exactly this entry, run the same command with the refs and --restored-before replaced by its ID, with GITHUB_TOKEN taken from the same source as for this command (a token is never printed): {api_prefix}velnor-ci cache delete-entry --repo {} --key {} --id {}\n",
        found.id,
        inert(&found.version),
        inert(&found.reference),
        shell_quote(&repo.to_string()),
        shell_quote(key),
        found.id
    )
}

pub(super) fn run(args: &DeleteEntryArgs) -> Result<String, CiError> {
    let (github, repo) = client(args.repo.as_deref())?;
    if let Some(id) = args.id {
        return delete_by_id(&github, &repo, &args.key, id);
    }
    let restored_before = args.restored_before.as_deref().unwrap_or_default();
    let found = resolve_entry(&github, &repo, &args.key, &args.refs, restored_before)?;
    Ok(resolved_report(&repo, &args.key, &found, &super::api_url()))
}
