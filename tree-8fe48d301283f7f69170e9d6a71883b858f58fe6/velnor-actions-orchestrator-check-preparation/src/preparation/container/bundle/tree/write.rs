//! Deadline-aware creation of the admitted immutable SDK tree.

use super::{Entry, Kind};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use velnor_actions_mise::CheckDeadline;
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::internal;

#[cfg(test)]
pub(in crate::preparation::container::bundle) fn populate(
    destination: &Path,
    entries: &[Entry],
) -> Result<(), OrchestratorError> {
    populate_until(destination, entries, None)
}

pub(in crate::preparation::container::bundle) fn populate_until(
    destination: &Path,
    entries: &[Entry],
    deadline: Option<CheckDeadline>,
) -> Result<(), OrchestratorError> {
    for entry in entries {
        super::checkpoint(deadline)?;
        if matches!(&entry.kind, Kind::Directory) {
            let path = destination.join(&entry.path);
            fs::create_dir(&path).map_err(|error| super::io_error(&path, error))?;
        }
    }
    for entry in entries {
        super::checkpoint(deadline)?;
        if let Kind::File {
            bytes, executable, ..
        } = &entry.kind
        {
            let path = destination.join(&entry.path);
            velnor_actions_orchestrator_core::exclusive_write::write_exclusive_until(
                &path,
                bytes,
                "sdk_file",
                || super::checkpoint(deadline),
            )?;
            set_mode(&path, readonly_mode(*executable), false)?;
        }
    }
    set_directory_mode(destination, 0o700)?;
    for entry in entries.iter().rev() {
        super::checkpoint(deadline)?;
        if matches!(&entry.kind, Kind::Directory) {
            set_directory_mode(&destination.join(&entry.path), 0o700)?;
        }
    }
    super::checkpoint(deadline)
}

pub(in crate::preparation::container::bundle) fn create_destination(
    path: &Path,
) -> Result<(), OrchestratorError> {
    super::super::reject_links(path.parent().unwrap_or_else(|| Path::new("/")))?;
    match fs::symlink_metadata(path) {
        Ok(_) => Err(OrchestratorError::OverwriteRefused {
            path: path.display().to_string(),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|error| super::io_error(path, error))
        }
        Err(error) => Err(super::io_error(path, error)),
    }
}

pub(in crate::preparation::container::bundle) fn set_directory_mode(
    path: &Path,
    value: u32,
) -> Result<(), OrchestratorError> {
    set_mode(path, value, true)
}

pub(in crate::preparation::container::bundle) fn verify_owned_root(
    path: &Path,
) -> Result<(), OrchestratorError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| super::io_error(path, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(super::unsafe_path(path, "sdk_owned_root_type"));
    }
    #[cfg(unix)]
    {
        let full_mode = metadata.permissions().mode();
        super::reject_special_mode(path, full_mode)?;
        if full_mode & 0o777 != 0o700 {
            return Err(internal("sdk_owned_root_mode"));
        }
    }
    Ok(())
}

fn set_mode(path: &Path, value: u32, directory: bool) -> Result<(), OrchestratorError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| super::io_error(path, error))?;
    if metadata.file_type().is_symlink()
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        return Err(super::unsafe_path(path, "sdk_output_type"));
    }
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(value & 0o777))
        .map_err(|error| super::io_error(path, error))?;
    #[cfg(not(unix))]
    let _ = value;
    Ok(())
}

fn readonly_mode(executable: bool) -> u32 {
    0o400 | if executable { 0o100 } else { 0 }
}
