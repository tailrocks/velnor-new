// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::cache_api::{Entry, client, delete_entries, pages};
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

fn matching_on<'a>(listed: &'a [Entry], key: &str, reference: &str) -> Vec<&'a Entry> {
    listed
        .iter()
        .filter(|entry| entry.key == key && entry.reference == reference)
        .collect()
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
    for reference in refs {
        let path = format!(
            "/repos/{repo}/actions/caches?key={}&ref={}",
            encode(key),
            encode(reference)
        );
        let (listed, _) = pages::<Entry>(github, &path, None)?;
        let on = inert(reference);
        match matching_on(&listed, key, reference).as_slice() {
            [] => {}
            [entry] => {
                delete_entries(github, repo, &[entry.id])?;
                return Ok(format!(
                    "deleted cache entry ID {} ({named}, version {}, {on})\n",
                    entry.id,
                    inert(&entry.version)
                ));
            }
            several => {
                return Err(CiError::new(format!(
                    "{} entries have the key {named} on {on}; nothing was deleted. Entries found: {}",
                    several.len(),
                    describe(several)
                )));
            }
        }
    }
    let searched: Vec<String> = refs.iter().map(|reference| inert(reference)).collect();
    Err(CiError::new(format!(
        "no cache entry has the key {named} on {}; nothing was deleted",
        searched.join(" or ")
    )))
}

pub(super) fn run(args: &DeleteEntryArgs) -> Result<String, CiError> {
    let (github, repo) = client(args.repo.as_deref())?;
    delete_entry(&github, &repo, &args.key, &args.refs)
}
