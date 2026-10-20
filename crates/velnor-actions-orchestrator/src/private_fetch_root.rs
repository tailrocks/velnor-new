//! Bundle root preparation for the private fetch (HD-7).

use std::path::{Path, PathBuf};

use crate::OrchestratorError;
use crate::config::private_fetch_session::scrub_dir;
use crate::internal::internal;

/// Workspace-relative bundle root, identical for local and CI fetches.
const DEPS_ROOT_RELPATH: &str = ".velnor/deps";

/// Prepare `<workspace>/.velnor/deps` for extraction.
///
/// The repository can place a file, directory, or symbolic link at this path.
/// Remove that entry without following symbolic links. Require `.velnor` to be a directory.
pub(crate) fn prepare_deps_root(workspace: &Path) -> Result<PathBuf, OrchestratorError> {
    let io = |path: &Path, err: std::io::Error| {
        OrchestratorError::io(path.display().to_string(), err.to_string())
    };
    // Resolve the workspace path before extraction. The archive reader rejects
    // symbolic links in parent directories.
    let workspace = std::fs::canonicalize(workspace).map_err(|err| io(workspace, err))?;
    let velnor = workspace.join(".velnor");
    match std::fs::symlink_metadata(&velnor) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Err(internal("private_deps_root_unsafe")),
        Err(err) => return Err(io(&velnor, err)),
    }
    let deps = workspace.join(DEPS_ROOT_RELPATH);
    match std::fs::symlink_metadata(&deps) {
        Ok(meta) if meta.is_dir() => scrub_dir(&deps)?,
        Ok(_) => std::fs::remove_file(&deps).map_err(|err| io(&deps, err))?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(io(&deps, err)),
    }
    Ok(deps)
}
