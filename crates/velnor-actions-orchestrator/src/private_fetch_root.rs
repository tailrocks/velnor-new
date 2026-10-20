//! Bundle root preparation for the private fetch (HD-7).

use std::path::{Path, PathBuf};

use crate::OrchestratorError;
use crate::config::private_fetch_session::scrub_dir;
use crate::internal::internal;

/// Workspace-relative bundle root, identical for local and CI fetches.
const DEPS_ROOT_RELPATH: &str = ".velnor/deps";

/// Reset `<workspace>/.velnor/deps` to an absent real directory path.
///
/// The checkout is repository-controlled, so the root may arrive as a file, a
/// directory with planted content or a symlink aimed outside the workspace.
/// Anything there is removed without following links, and `.velnor` itself must
/// be a real directory, so extraction can only land inside the workspace.
pub(crate) fn prepare_deps_root(workspace: &Path) -> Result<PathBuf, OrchestratorError> {
    let io = |path: &Path, err: std::io::Error| {
        OrchestratorError::io(path.display().to_string(), err.to_string())
    };
    // The archive reader refuses any symlinked ancestor, so resolve the
    // runner-provided workspace spelling (for example `/var` -> `/private/var`) once.
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
