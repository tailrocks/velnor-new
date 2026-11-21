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

pub fn delete_entry(
    github: &dyn GitHubWrite,
    repo: &Repo,
    key: &str,
    version: &str,
    reference: Option<&str>,
) -> Result<String, CiError> {
    if key.is_empty() || version.is_empty() {
        return Err(CiError::new("pass the exact cache key and its version"));
    }
    let by_ref = reference.map_or_else(String::new, |reference| {
        format!("&ref={}", encode(reference))
    });
    let path = format!("/repos/{repo}/actions/caches?key={}{by_ref}", encode(key));
    let (listed, _) = pages::<Entry>(github, &path, None)?;
    let same_key: Vec<&Entry> = listed.iter().filter(|entry| entry.key == key).collect();
    let matching: Vec<&Entry> = same_key
        .iter()
        .copied()
        .filter(|entry| entry.version == version)
        .collect();
    let named = inert(key);
    match matching.as_slice() {
        [entry] => {
            delete_entries(github, repo, &[entry.id])?;
            Ok(format!(
                "deleted cache entry ID {} ({named}, version {version}, {})\n",
                entry.id,
                inert(&entry.reference)
            ))
        }
        [] if same_key.is_empty() => Err(CiError::new(format!(
            "no cache entry has the key {named}; nothing was deleted"
        ))),
        [] => Err(CiError::new(format!(
            "no entry with the key {named} has version {version}; nothing was deleted. The entries with that key are: {}",
            describe(&same_key)
        ))),
        several => Err(CiError::new(format!(
            "{} entries have the key {named} and version {version}; nothing was deleted. Pass --ref to choose one ref: {}",
            several.len(),
            describe(several)
        ))),
    }
}

pub(super) fn run(args: &DeleteEntryArgs) -> Result<String, CiError> {
    let (github, repo) = client(args.repo.as_deref())?;
    delete_entry(
        &github,
        &repo,
        &args.key,
        &args.version,
        args.reference.as_deref(),
    )
}
