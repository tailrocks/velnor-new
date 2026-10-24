use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use crate::error::HostError;

/// Resolve a journal path without following a journal-file symlink.
///
/// # Errors
///
/// Returns [`HostError::Path`] unless the parent exists and the path names a
/// regular file or a new file in that parent.
pub(crate) fn canonical_journal_path(path: &Path) -> Result<PathBuf, HostError> {
    if !path.is_absolute() || path.file_name().is_none() {
        return Err(HostError::Path);
    }
    let parent = path.parent().ok_or(HostError::Path)?;
    let parent = fs::canonicalize(parent).map_err(|_| HostError::Path)?;
    if !fs::metadata(&parent).map_err(|_| HostError::Path)?.is_dir() {
        return Err(HostError::Path);
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.chars().any(char::is_control))
        .ok_or(HostError::Path)?;
    let canonical = parent.join(name);
    if canonical
        .to_str()
        .is_none_or(|text| text.chars().any(char::is_control))
    {
        return Err(HostError::Path);
    }
    match fs::symlink_metadata(&canonical) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(HostError::Path)
        }
        Ok(_) => Ok(canonical),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(canonical),
        Err(_) => Err(HostError::Path),
    }
}

pub(super) fn private_directory(path: &Path) -> Result<PathBuf, HostError> {
    if !path.is_absolute() {
        return Err(HostError::Path);
    }
    fs::create_dir_all(path).map_err(|_| HostError::Path)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| HostError::Path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(HostError::Path);
    }
    let canonical = fs::canonicalize(path).map_err(|_| HostError::Path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&canonical, fs::Permissions::from_mode(0o700))
            .map_err(|_| HostError::Path)?;
    }
    Ok(canonical)
}

pub(super) fn open_lock(path: &Path) -> Result<File, HostError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| HostError::Lock)?;
    }
    ensure_regular_or_missing(path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path).map_err(|_| HostError::Lock)?;
    if !file.metadata().map_err(|_| HostError::Lock)?.is_file() {
        return Err(HostError::Path);
    }
    ensure_regular_or_missing(path)?;
    Ok(file)
}

pub(super) fn ensure_regular_or_missing(path: &Path) -> Result<(), HostError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(HostError::Path)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(HostError::Lock),
    }
}
