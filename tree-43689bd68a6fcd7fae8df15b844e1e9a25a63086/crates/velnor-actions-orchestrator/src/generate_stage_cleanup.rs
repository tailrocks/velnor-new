//! Remove staged or retired entries without unlinking their persistent root.

use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};

use crate::OrchestratorError;

const CLEANUP_DIRECTORY_MODE: u32 = 0o700;

/// Delete every child under a verified root, preserving the root directory.
pub(super) fn clear_children(root: &Path) -> Result<(), OrchestratorError> {
    let metadata = fs::symlink_metadata(root).map_err(|error| io(root, &error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(unsafe_path(root, "cleanup_root_not_directory"));
    }
    set_directory_mode(root)?;
    let mut pending: Vec<(PathBuf, bool)> = children(root)?
        .into_iter()
        .map(|path| (path, false))
        .collect();
    while let Some((path, visited)) = pending.pop() {
        clear_entry(&path, visited, &mut pending)?;
    }
    Ok(())
}

/// Process one path, visiting real directories before removing them.
fn clear_entry(
    path: &Path,
    visited: bool,
    pending: &mut Vec<(PathBuf, bool)>,
) -> Result<(), OrchestratorError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| io(path, &error))?;
    if metadata.file_type().is_symlink() {
        return remove_link(path, &metadata);
    }
    if !metadata.is_dir() {
        return fs::remove_file(path).map_err(|error| io(path, &error));
    }
    if visited {
        return fs::remove_dir(path).map_err(|error| io(path, &error));
    }
    set_directory_mode(path)?;
    pending.push((path.to_path_buf(), true));
    pending.extend(children(path)?.into_iter().map(|child| (child, false)));
    Ok(())
}

/// Enumerate direct children without resolving any symbolic links.
fn children(path: &Path) -> Result<Vec<PathBuf>, OrchestratorError> {
    let entries = fs::read_dir(path).map_err(|error| io(path, &error))?;
    entries
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| io(path, &error))
        })
        .collect()
}

/// Remove a symbolic link itself, including a directory link on Windows.
fn remove_link(path: &Path, metadata: &Metadata) -> Result<(), OrchestratorError> {
    #[cfg(unix)]
    {
        let _ = metadata;
        fs::remove_file(path).map_err(|error| io(path, &error))
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;
        let result = if metadata.file_type().is_symlink_dir() {
            fs::remove_dir(path)
        } else {
            fs::remove_file(path)
        };
        result.map_err(|error| io(path, &error))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = metadata;
        fs::remove_file(path).map_err(|error| io(path, &error))
    }
}

fn set_directory_mode(path: &Path) -> Result<(), OrchestratorError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(CLEANUP_DIRECTORY_MODE))
            .map_err(|error| io(path, &error))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

fn io(path: &Path, error: &std::io::Error) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), error.to_string())
}

fn unsafe_path(path: &Path, reason: &str) -> OrchestratorError {
    OrchestratorError::UnsafePath {
        path: path.display().to_string(),
        reason: reason.to_owned(),
    }
}
