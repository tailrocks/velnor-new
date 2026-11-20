// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::GitRepository;

use crate::cargo_lock::CargoLock;
use crate::lock::LockError;

const PUBLIC_REGISTRIES: [&str; 2] = [
    "registry+https://github.com/rust-lang/crates.io-index",
    "sparse+https://index.crates.io/",
];
const GITHUB_PREFIX: &str = "git+https://github.com/";
const COMMIT_LENGTH: usize = 40;

/// A locked git source that a lock entry pins to one full commit on github.com over https.
pub(crate) struct PinnedGithub<'a> {
    pub(crate) owner: &'a str,
    pub(crate) name: &'a str,
}

pub(crate) fn pinned_github(source: &str) -> Option<PinnedGithub<'_>> {
    let (location, commit) = source.strip_prefix(GITHUB_PREFIX)?.split_once('#')?;
    if commit.len() != COMMIT_LENGTH
        || !commit
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return None;
    }
    let path = location.split_once('?').map_or(location, |(path, _)| path);
    let (owner, name) = path.split_once('/')?;
    let name = name.strip_suffix(".git").unwrap_or(name);
    (slug(owner) && slug(name)).then_some(PinnedGithub { owner, name })
}

pub(crate) fn slug(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with('.')
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// What a `Cargo.lock` allows before any network check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockAdmission {
    /// Every source is crates.io or a path: nothing more to prove.
    Public,
    /// Some source is not a public one, or the lock has a patch.
    Refused { source: String },
    /// Only crates.io, paths and commit-pinned github.com sources: cacheable once each
    /// repository answers a token-free public lookup.
    ProbeGithub(Vec<GitRepository>),
}

/// # Errors
/// Returns an error when the lock cannot be read as a Cargo lock.
pub fn lock_admission(text: &str) -> Result<LockAdmission, LockError> {
    let lock = CargoLock::parse(text)?;
    if lock.patch.is_some() {
        return Ok(LockAdmission::Refused {
            source: "[patch]".to_owned(),
        });
    }
    let mut repositories: Vec<GitRepository> = Vec::new();
    for package in &lock.packages {
        let Some(source) = package.source.as_deref() else {
            continue;
        };
        if PUBLIC_REGISTRIES.contains(&source) {
            continue;
        }
        let Some(git) = pinned_github(source) else {
            return Ok(LockAdmission::Refused {
                source: format!("{} from {source}", package.name),
            });
        };
        if !repositories
            .iter()
            .any(|known| known.matches(git.owner, git.name))
        {
            repositories.push(GitRepository {
                owner: git.owner.to_owned(),
                name: git.name.to_owned(),
            });
        }
    }
    repositories.sort();
    Ok(if repositories.is_empty() {
        LockAdmission::Public
    } else {
        LockAdmission::ProbeGithub(repositories)
    })
}
