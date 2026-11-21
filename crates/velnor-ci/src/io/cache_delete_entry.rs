// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::cache_api::{Entry, client, delete_entries, exact_entries};
use crate::cache::encode;
use crate::cli_cache::DeleteEntryArgs;
use crate::error::CiError;
use crate::escape::inert;
use crate::github::{GitHubWrite, Repo};

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

pub fn delete_entry(
    github: &dyn GitHubWrite,
    repo: &Repo,
    key: &str,
    refs: &[String],
) -> Result<String, CiError> {
    if key.is_empty() || refs.is_empty() || refs.iter().any(String::is_empty) {
        return Err(CiError::new("pass the exact cache key and its refs"));
    }
    let named = inert(key);
    let held = holders(github, repo, key, refs)?;
    let Some(first) = held.first() else {
        let searched: Vec<String> = refs.iter().map(|reference| inert(reference)).collect();
        return Err(CiError::new(format!(
            "no cache entry has the key {named} on {}; nothing was deleted",
            searched.join(" or ")
        )));
    };
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
            "the key {named} is held under more than one version, and the restore may have used any ref that holds it; nothing was deleted. Entries found: {}. Check which version the job used, then delete that entry by its ID with DELETE /repos/{repo}/actions/caches/<ID>",
            describe(&all)
        )));
    }
    delete_entries(github, repo, &[first.id])?;
    Ok(format!(
        "deleted cache entry ID {} ({named}, version {}, {})\n",
        first.id,
        inert(&first.version),
        inert(&first.reference)
    ))
}

pub(super) fn run(args: &DeleteEntryArgs) -> Result<String, CiError> {
    let (github, repo) = client(args.repo.as_deref())?;
    delete_entry(&github, &repo, &args.key, &args.refs)
}
