//! Where fetched private bundles live in the workspace.
//!
//! One source of truth shared by the writer (the private fetch) and the
//! readers (for example `gradle-run-v1`), so they cannot drift apart.

use std::path::{Path, PathBuf};

/// Workspace-relative bundle root, identical for local and CI fetches.
const DEPS_ROOT_RELPATH: &str = ".velnor/deps";
/// Directory under the bundle root that holds one unpacked Maven bundle per id.
const MAVEN_DIR: &str = "maven";

/// The bundle root of a workspace: the one place fetch writes and consumers read.
pub(crate) fn deps_root(workspace: &Path) -> PathBuf {
    workspace.join(DEPS_ROOT_RELPATH)
}

/// The Maven bundle root under a bundle root (`<deps>/maven`).
pub(crate) fn maven_root(deps: &Path) -> PathBuf {
    deps.join(MAVEN_DIR)
}

/// Where one fetched Maven bundle is unpacked (`<deps>/maven/<id>`).
pub(crate) fn maven_bundle_dir(deps: &Path, id: &str) -> PathBuf {
    maven_root(deps).join(id)
}
