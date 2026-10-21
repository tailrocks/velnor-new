//! `.velnor/deps/verified.json`: what one private fetch verified (HD-7).
//!
//! `private-fetch-v1` writes this file only after it verifies and extracts every requested bundle and tree.
//! Repository code can compare the file with `.velnor/deps.lock` without another fetch.
//! Repository code can also change the file, so its presence alone does not prove fetch provenance.
//! See "Verified marker" in `docs/proposed/private-sources-contract.md` for the stable format.

use std::path::Path;

use serde::Serialize;
use velnor_actions_contract::deps_lock::{LockedAsset, LockedGitTree};

use crate::OrchestratorError;
use crate::config::private_sources::SourceKind;
use crate::internal::internal;

/// File name under the bundle root (`<workspace>/.velnor/deps/verified.json`).
pub(crate) const VERIFIED_FILE: &str = "verified.json";
/// Marker schema version; bumped only with a breaking format change.
pub(crate) const VERIFIED_SCHEMA: u32 = 1;

/// The marker document.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub(crate) struct Verified {
    schema: u32,
    sources: Vec<VerifiedSource>,
}

/// One verified bundle or tree.
#[derive(Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct VerifiedSource {
    /// `maven-bundles`, `release-assets` or `git-trees`.
    kind: &'static str,
    /// Source id from `.velnor/config.toml` and the lock.
    id: String,
    /// Extracted directory, relative to `.velnor/deps`, `/`-separated.
    path: String,
    /// Producer repository as `Owner/name`.
    repository: String,
    /// Tag the lock pins.
    tag: String,
    /// Commit the tag resolved to, checked against the lock before download.
    commit: String,
    /// Release asset file name (absent for git trees).
    #[serde(skip_serializing_if = "Option::is_none")]
    asset: Option<String>,
    /// SHA-256 of the downloaded archive bytes, equal to the lock (absent for
    /// git trees, which the commit pins).
    #[serde(skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
}

impl VerifiedSource {
    pub(crate) fn asset(kind: SourceKind, asset: &LockedAsset, path: &str) -> Self {
        Self {
            kind: kind.as_str(),
            id: asset.id.clone(),
            path: path.to_owned(),
            repository: asset.repository.clone(),
            tag: asset.tag.clone(),
            commit: asset.commit.clone(),
            asset: Some(asset.asset.clone()),
            sha256: Some(asset.sha256.clone()),
        }
    }

    pub(crate) fn tree(tree: &LockedGitTree, path: &str) -> Self {
        Self {
            kind: SourceKind::GitTrees.as_str(),
            id: tree.id.clone(),
            path: path.to_owned(),
            repository: tree.repository.clone(),
            tag: tree.tag.clone(),
            commit: tree.commit.clone(),
            asset: None,
            sha256: None,
        }
    }
}

impl Verified {
    /// Deterministic document: sources sorted by kind, then id.
    pub(crate) fn new(mut sources: Vec<VerifiedSource>) -> Self {
        sources.sort();
        Self {
            schema: VERIFIED_SCHEMA,
            sources,
        }
    }

    /// Pretty JSON with a trailing newline.
    pub(crate) fn render(&self) -> Result<String, OrchestratorError> {
        serde_json::to_string_pretty(self)
            .map(|text| text + "\n")
            .map_err(|err| internal(&format!("private_verified_render:{err}")))
    }

    /// Write the marker into the bundle root.
    /// An empty fetch writes no marker. A repository with only Cargo sources gets no workspace file.
    ///
    /// # Errors
    /// The function fails if a bundle occupies the marker path or a file operation fails.
    pub(crate) fn write(&self, deps: &Path) -> Result<(), OrchestratorError> {
        if self.sources.is_empty() {
            return Ok(());
        }
        let path = deps.join(VERIFIED_FILE);
        if std::fs::symlink_metadata(&path).is_ok() {
            return Err(internal("private_verified_marker_collision"));
        }
        let io = |p: &Path, err: std::io::Error| {
            OrchestratorError::io(p.display().to_string(), err.to_string())
        };
        let partial = deps.join(format!(".{VERIFIED_FILE}.partial"));
        std::fs::write(&partial, self.render()?).map_err(|err| io(&partial, err))?;
        std::fs::rename(&partial, &path).map_err(|err| io(&path, err))
    }
}
