use std::path::{Path, PathBuf};

const DEPS_ROOT_RELPATH: &str = ".velnor/deps";
const MAVEN_DIR: &str = "maven";

pub(crate) fn deps_root(workspace: &Path) -> PathBuf {
    workspace.join(DEPS_ROOT_RELPATH)
}

pub(crate) fn maven_root(deps: &Path) -> PathBuf {
    deps.join(MAVEN_DIR)
}

pub(crate) fn maven_bundle_dir(deps: &Path, id: &str) -> PathBuf {
    maven_root(deps).join(id)
}
