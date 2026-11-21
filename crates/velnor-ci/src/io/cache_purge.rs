// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use super::cache_api::{Entry, delete_entries, pages};
use crate::cache::encode;
use crate::cli_cache::PurgePrivateArgs;
use crate::error::CiError;
use crate::github::{GitHubWrite, Repo};

const PRIVATE_PREFIX: &str = "velnor-private-";

pub fn purge_private(
    github: &dyn GitHubWrite,
    repo: &Repo,
    delete: bool,
) -> Result<String, CiError> {
    let path = format!(
        "/repos/{repo}/actions/caches?key={}",
        encode(PRIVATE_PREFIX)
    );
    let (listed, _) = pages::<Entry>(github, &path, None)?;
    let private: Vec<_> = listed
        .iter()
        .filter(|entry| entry.key.starts_with(PRIVATE_PREFIX))
        .collect();
    let ids: BTreeSet<u64> = private.iter().map(|entry| entry.id).collect();
    if ids.len() != private.len() {
        return Err(CiError::new("the cache listing repeats a cache ID"));
    }
    let bytes: u128 = private
        .iter()
        .map(|entry| u128::from(entry.size_in_bytes))
        .sum();
    let refs: BTreeSet<_> = private.iter().map(|entry| &entry.reference).collect();
    let ids: Vec<u64> = ids.into_iter().collect();
    let mut report = format!(
        "{} entries ({bytes} bytes) in {} refs of {repo} have a key starting with {PRIVATE_PREFIX}\n",
        ids.len(),
        refs.len()
    );
    if delete {
        delete_entries(github, repo, &ids)?;
        Ok(format!("{report}deleted {} entries\n", ids.len()))
    } else {
        Ok(format!(
            "{report}would delete {} entries; pass --yes to delete them\n",
            ids.len()
        ))
    }
}

pub(super) fn run(args: &PurgePrivateArgs) -> Result<String, CiError> {
    let (github, repo) = super::cache_api::client(args.repo.as_deref())?;
    purge_private(&github, &repo, args.yes)
}
