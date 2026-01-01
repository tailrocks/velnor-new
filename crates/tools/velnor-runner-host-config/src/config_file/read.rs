//! Protected, bounded reads from one opened configuration file.

use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use rustix::fs::{Mode, OFlags, open, openat};
use rustix::io::Errno;
use velnor_runner_journal::HostError;

use super::{HostPlatform, linux_service_group_id, safe_macos_directory, safe_macos_file};

/// Maximum encoded host configuration size accepted from disk.
pub const MAX_HOST_CONFIG_BYTES: usize = 64 * 1024;

/// Read a protected configuration file as its exact bytes.
///
/// The parent and leaf are opened without following their final symlink, and
/// the leaf is opened nonblocking before its type is inspected. This prevents
/// a FIFO or device at the config path from blocking the daemon before it can
/// reject the object. Reads are capped before parsing or hashing.
///
/// # Errors
///
/// Returns [`HostError::Config`] when ownership, mode, file type, encoding
/// size, or any read check fails.
pub fn read_host_config_bytes(
    path: &Path,
    platform: HostPlatform,
) -> Result<Option<Vec<u8>>, HostError> {
    match platform {
        HostPlatform::Linux => {
            super::secure_read::read_linux_config(path, linux_service_group_id()?)
        }
        HostPlatform::Macos => read_macos(path),
    }
}

fn read_macos(path: &Path) -> Result<Option<Vec<u8>>, HostError> {
    let parent = path.parent().ok_or(HostError::Config)?;
    match fs::symlink_metadata(parent) {
        Ok(metadata) if safe_macos_directory(&metadata)? => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Ok(_) | Err(_) => return Err(HostError::Config),
    }
    let Some(directory) = open_directory(parent)? else {
        return Ok(None);
    };
    if !safe_macos_directory(&directory.metadata().map_err(|_| HostError::Config)?)? {
        return Err(HostError::Config);
    }
    read_leaf(
        &directory,
        path.file_name().ok_or(HostError::Config)?,
        |metadata| Ok(metadata.is_file() && safe_macos_file(metadata)?),
    )
}

fn open_directory(path: &Path) -> Result<Option<File>, HostError> {
    match open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    ) {
        Ok(fd) => Ok(Some(File::from(fd))),
        Err(Errno::NOENT) => Ok(None),
        Err(_) => Err(HostError::Config),
    }
}

fn read_leaf(
    directory: &File,
    name: &OsStr,
    validate_file: impl FnOnce(&fs::Metadata) -> Result<bool, HostError>,
) -> Result<Option<Vec<u8>>, HostError> {
    let fd = match openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(Errno::NOENT) => return Ok(None),
        Err(_) => return Err(HostError::Config),
    };
    let mut file = File::from(fd);
    let metadata = file.metadata().map_err(|_| HostError::Config)?;
    if !metadata.is_file()
        || !validate_file(&metadata)?
        || metadata.len() > MAX_HOST_CONFIG_BYTES as u64
    {
        return Err(HostError::Config);
    }
    let capacity = usize::try_from(metadata.len()).map_err(|_| HostError::Config)?;
    let mut bytes = Vec::with_capacity(capacity);
    file.by_ref()
        .take((MAX_HOST_CONFIG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| HostError::Config)?;
    if bytes.len() > MAX_HOST_CONFIG_BYTES {
        return Err(HostError::Config);
    }
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests;
