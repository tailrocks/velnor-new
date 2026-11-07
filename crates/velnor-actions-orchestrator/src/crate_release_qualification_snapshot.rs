// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, crate_release_git::ReleaseGit, internal::internal};
use std::path::Path;

#[derive(Debug)]
pub struct SourceSnapshot {
    directory: tempfile::TempDir,
    source_sha: String,
    tree_sha: String,
    manifests: std::collections::BTreeSet<String>,
    nested_toolchains: Vec<String>,
}

impl SourceSnapshot {
    #[must_use]
    pub(crate) fn root(&self) -> &Path {
        self.directory.path()
    }
    #[must_use]
    pub fn source_sha(&self) -> &str {
        &self.source_sha
    }
    #[must_use]
    pub fn tree_sha(&self) -> &str {
        &self.tree_sha
    }
    pub(crate) fn manifests(&self) -> &std::collections::BTreeSet<String> {
        &self.manifests
    }
    pub(crate) fn nested_toolchains(&self) -> &[String] {
        &self.nested_toolchains
    }
}

#[path = "crate_release_qualification_object.rs"]
pub(crate) mod object;

pub fn snapshot_approved_source(
    source: &Path,
    sha: &str,
    git: &ReleaseGit,
) -> Result<SourceSnapshot, OrchestratorError> {
    crate::verify_crate_release_source(source, sha, git)?;
    let deadline = velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(90))
        .map_err(|_| invalid())?;
    let snapshot = object::snapshot(source, sha, git, deadline)?;
    crate::verify_crate_release_source(source, sha, git)?;
    Ok(snapshot)
}

pub(crate) fn snapshot_git_objects(
    source: &Path,
    sha: &str,
    git: &ReleaseGit,
) -> Result<SourceSnapshot, OrchestratorError> {
    let deadline = velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(90))
        .map_err(|_| invalid())?;
    snapshot_git_objects_until(source, sha, git, deadline)
}

pub(crate) fn snapshot_git_objects_until(
    source: &Path,
    sha: &str,
    git: &ReleaseGit,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<SourceSnapshot, OrchestratorError> {
    object::snapshot(source, sha, git, deadline)
}

#[cfg(unix)]
fn create_link(path: &Path, target: &str) -> Result<(), OrchestratorError> {
    std::os::unix::fs::symlink(target, path).map_err(|_| invalid())
}
#[cfg(not(unix))]
fn create_link(_path: &Path, _target: &str) -> Result<(), OrchestratorError> {
    Err(invalid())
}

#[cfg(unix)]
fn set_executable(path: &Path, executable: bool) -> Result<(), OrchestratorError> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(
        path,
        std::fs::Permissions::from_mode(if executable { 0o700 } else { 0o600 }),
    )
    .map_err(|_| invalid())
}
#[cfg(not(unix))]
fn set_executable(_path: &Path, _executable: bool) -> Result<(), OrchestratorError> {
    Err(invalid())
}

fn invalid() -> OrchestratorError {
    internal("qualification_source_snapshot_invalid")
}
