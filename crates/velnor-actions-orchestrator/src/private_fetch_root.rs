//! Bundle root preparation and extraction for the private fetch (HD-7).

use std::path::{Path, PathBuf};
use std::time::Duration;

use velnor_actions_mise::CheckDeadline;

use crate::OrchestratorError;
use crate::check_runtime::preparation::acquisition::archive::{ArchiveBudget, extract_archive};
use crate::config::deps_layout::deps_root;
use crate::config::private_fetch_session::scrub_dir;
use crate::internal::internal;

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
    let deps = deps_root(&workspace);
    match std::fs::symlink_metadata(&deps) {
        Ok(meta) if meta.is_dir() => scrub_dir(&deps)?,
        Ok(_) => std::fs::remove_file(&deps).map_err(|err| io(&deps, err))?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(io(&deps, err)),
    }
    Ok(deps)
}

const ARCHIVE_DEADLINE: Duration = Duration::from_mins(30);

/// Extract one verified archive through the bounded reader into `destination`.
pub(crate) fn extract_into(
    archive: &Path,
    destination: &Path,
    budget: &mut ArchiveBudget,
) -> Result<(), OrchestratorError> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| OrchestratorError::io(parent.display().to_string(), err.to_string()))?;
    }
    let name = archive
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let deadline =
        CheckDeadline::after(ARCHIVE_DEADLINE).map_err(|err| internal(&err.to_string()))?;
    extract_archive(archive, destination, &name, budget, deadline)
}
