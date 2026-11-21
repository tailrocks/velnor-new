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

pub fn purge_private(
    github: &dyn GitHubWrite,
    repo: &Repo,
    delete: bool,
) -> Result<String, CiError> {
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
        return Ok(format!(
            "{report}would delete {} entries; pass --yes to delete them\n",
            pending.len()
        ));
    }
    let mut deleted = 0;
    for _ in 0..PURGE_PASSES {
        let ids: Vec<u64> = pending.iter().map(|entry| entry.id).collect();
        for (done, id) in ids.iter().enumerate() {
            if let Err(error) = delete_entries(github, repo, &[*id]) {
                return Err(CiError::new(format!(
                    "deleted {done} of {}; failed at entry ID {id}: {error}",
                    ids.len()
                )));
            }
        }
        deleted += ids.len();
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
