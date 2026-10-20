//! Private source closure shared by the generator and the fetch operation (HD-7).
//!
//! The generator renders the token allowlist literally into the mint step; the
//! fetch operation recomputes the same set from the checked-out data and refuses
//! to run on any difference, so a pull request that edits a lock to name another
//! repository fails before the token is used.

use std::collections::BTreeSet;
use std::path::Path;

use serde::Deserialize;
use velnor_actions_contract::config::PrivateSourcesConfig;
use velnor_actions_contract::deps_lock::DepsLock;

use crate::OrchestratorError;
use crate::discover::workspace_lock;
use crate::internal::internal;
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};

/// One kind of private source a fetch step retrieves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SourceKind {
    /// Cargo git dependencies named by `Cargo.lock`.
    Cargo,
    /// Git trees archived from tags.
    GitTrees,
    /// Maven bundles shipped as release assets.
    MavenBundles,
    /// Plain release assets.
    ReleaseAssets,
}

impl SourceKind {
    /// Canonical spelling shared with the renderer's `PRIVATE_SOURCE_KINDS`.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::GitTrees => "git-trees",
            Self::MavenBundles => "maven-bundles",
            Self::ReleaseAssets => "release-assets",
        }
    }

    /// Parse one canonical spelling.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        [
            Self::Cargo,
            Self::GitTrees,
            Self::MavenBundles,
            Self::ReleaseAssets,
        ]
        .into_iter()
        .find(|kind| kind.as_str() == text)
    }
}

#[derive(Deserialize)]
struct CargoLock {
    #[serde(default)]
    package: Vec<LockedPackage>,
}

#[derive(Deserialize)]
struct LockedPackage {
    source: Option<String>,
}

/// Repository names under `owner` that one workspace root's `Cargo.lock` pulls from git.
///
/// # Errors
/// Rejects an unreadable, oversize or malformed lock.
pub(crate) fn cargo_lock_repositories(
    workspace: &Path,
    root: &str,
    owner: &str,
) -> Result<BTreeSet<String>, OrchestratorError> {
    let rel = workspace_lock(root);
    let text = match read_repo_file(workspace, &rel, MAX_REPO_FILE_BYTES)? {
        RepoRead::Absent => return Ok(BTreeSet::new()),
        RepoRead::Text(text) => text,
    };
    let lock: CargoLock = toml::from_str(&text)
        .map_err(|_| internal(&format!("private_cargo_lock_malformed:{rel}")))?;
    let sources = lock
        .package
        .iter()
        .filter_map(|package| package.source.as_deref());
    Ok(
        velnor_actions_contract::deps_lock::token_repository_allowlist(owner, sources, None)
            .into_iter()
            .collect(),
    )
}

/// Repository names the locked sources of the requested kinds read.
pub(crate) fn lock_repositories(
    lock: Option<&DepsLock>,
    owner: &str,
    kinds: &BTreeSet<SourceKind>,
) -> BTreeSet<String> {
    let Some(lock) = lock else {
        return BTreeSet::new();
    };
    let mut repositories = BTreeSet::new();
    let mut add = |repository: &str| {
        if let Some((found, name)) = velnor_actions_contract::config::split_repository(repository)
            && found == owner
        {
            repositories.insert(name.to_owned());
        }
    };
    if kinds.contains(&SourceKind::MavenBundles) {
        lock.maven_bundles.iter().for_each(|a| add(&a.repository));
    }
    if kinds.contains(&SourceKind::ReleaseAssets) {
        lock.release_assets.iter().for_each(|a| add(&a.repository));
    }
    if kinds.contains(&SourceKind::GitTrees) {
        lock.git_trees.iter().for_each(|t| add(&t.repository));
    }
    repositories
}

/// The token allowlist for one job: Cargo sources of its roots plus the locked
/// sources of the requested kinds, sorted by name.
///
/// # Errors
/// Propagates lock read failures.
pub(crate) fn job_allowlist(
    workspace: &Path,
    config: &PrivateSourcesConfig,
    lock: Option<&DepsLock>,
    roots: &[String],
    kinds: &BTreeSet<SourceKind>,
) -> Result<Vec<String>, OrchestratorError> {
    let mut names = lock_repositories(lock, &config.owner, kinds);
    if kinds.contains(&SourceKind::Cargo) {
        for root in roots {
            names.extend(cargo_lock_repositories(workspace, root, &config.owner)?);
        }
    }
    Ok(names.into_iter().collect())
}

#[cfg(test)]
#[path = "private_sources_tests.rs"]
mod tests;
