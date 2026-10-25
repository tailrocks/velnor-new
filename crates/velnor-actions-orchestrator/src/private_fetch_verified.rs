use std::path::Path;

use serde::Serialize;
use velnor_actions_contract::deps_lock::{LockedAsset, LockedGitTree};

use crate::OrchestratorError;
use crate::config::private_sources::SourceKind;
use crate::internal::internal;

pub(crate) const VERIFIED_FILE: &str = "verified.json";
pub(crate) const VERIFIED_SCHEMA: u32 = 1;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub(crate) struct Verified {
    schema: u32,
    sources: Vec<VerifiedSource>,
}

#[derive(Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct VerifiedSource {
    kind: &'static str,
    id: String,
    path: String,
    repository: String,
    tag: String,
    commit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    asset: Option<String>,
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
    pub(crate) fn new(mut sources: Vec<VerifiedSource>) -> Self {
        sources.sort();
        Self {
            schema: VERIFIED_SCHEMA,
            sources,
        }
    }

    pub(crate) fn render(&self) -> Result<String, OrchestratorError> {
        serde_json::to_string_pretty(self)
            .map(|text| text + "\n")
            .map_err(|err| internal(&format!("private_verified_render:{err}")))
    }

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
