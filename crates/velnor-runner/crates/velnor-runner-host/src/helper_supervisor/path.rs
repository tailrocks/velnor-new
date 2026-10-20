//! Fixed adjacent verifier resolution and byte-identity checks.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::HostError;

pub(super) const HELPER_NAME: &str = "velnor-runner-attestation-helper";
pub(super) const MAX_HELPER_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug)]
pub(super) struct VerifiedHelper {
    pub(super) file: File,
    pub(super) length: u64,
}

pub(super) fn open_verified(expected_sha256: &[u8; 32]) -> Result<VerifiedHelper, HostError> {
    let executable = std::env::current_exe().map_err(|_| HostError::Identity)?;
    open_verified_from(&executable, expected_sha256)
}

pub(super) fn open_verified_from(
    executable: &Path,
    expected_sha256: &[u8; 32],
) -> Result<VerifiedHelper, HostError> {
    let canonical = std::fs::canonicalize(executable).map_err(|_| HostError::Identity)?;
    if executable != canonical.as_path() || !safe_components(&canonical) {
        return Err(HostError::Identity);
    }
    let parent = canonical.parent().ok_or(HostError::Identity)?;
    let expected_owner = executable_owner(&canonical)?;
    validate_directory(parent, expected_owner)?;
    let helper = parent.join(HELPER_NAME);
    let mut file = open_regular(&helper, expected_owner)?;
    let metadata = file.metadata().map_err(|_| HostError::Identity)?;
    if metadata.len() == 0 || metadata.len() > MAX_HELPER_BYTES {
        return Err(HostError::Identity);
    }
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut read_total = 0_u64;
    loop {
        let read = file.read(&mut buffer).map_err(|_| HostError::Identity)?;
        if read == 0 {
            break;
        }
        read_total = read_total
            .checked_add(u64::try_from(read).map_err(|_| HostError::Identity)?)
            .filter(|total| *total <= MAX_HELPER_BYTES)
            .ok_or(HostError::Identity)?;
        digest.update(&buffer[..read]);
    }
    if read_total != metadata.len() || digest.finalize().as_slice() != expected_sha256 {
        return Err(HostError::Identity);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| HostError::Identity)?;
    Ok(VerifiedHelper {
        file,
        length: metadata.len(),
    })
}

fn safe_components(path: &Path) -> bool {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => current.push(component.as_os_str()),
            Component::Normal(part) => {
                current.push(part);
                if std::fs::symlink_metadata(&current)
                    .map_or(true, |metadata| metadata.file_type().is_symlink())
                {
                    return false;
                }
            }
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => return false,
        }
    }
    true
}

#[cfg(unix)]
fn validate_directory(path: &Path, expected_owner: u32) -> Result<(), HostError> {
    use std::os::unix::fs::MetadataExt;

    let metadata = std::fs::symlink_metadata(path).map_err(|_| HostError::Identity)?;
    if !metadata.is_dir() || metadata.uid() != expected_owner || metadata.mode() & 0o022 != 0 {
        return Err(HostError::Identity);
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_directory(_path: &Path, _expected_owner: u32) -> Result<(), HostError> {
    Err(HostError::Identity)
}

#[cfg(unix)]
fn open_regular(path: &Path, expected_owner: u32) -> Result<File, HostError> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    let metadata = std::fs::symlink_metadata(path).map_err(|_| HostError::Identity)?;
    if !metadata.is_file()
        || metadata.uid() != expected_owner
        || metadata.mode() & 0o022 != 0
        || metadata.mode() & 0o111 == 0
    {
        return Err(HostError::Identity);
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| HostError::Identity)?;
    let opened = file.metadata().map_err(|_| HostError::Identity)?;
    if !opened.is_file() || opened.uid() != expected_owner || opened.mode() & 0o022 != 0 {
        return Err(HostError::Identity);
    }
    Ok(file)
}

#[cfg(not(unix))]
fn open_regular(_path: &Path, _expected_owner: u32) -> Result<File, HostError> {
    Err(HostError::Identity)
}

#[cfg(unix)]
fn executable_owner(path: &Path) -> Result<u32, HostError> {
    use std::os::unix::fs::MetadataExt;

    let metadata = std::fs::symlink_metadata(path).map_err(|_| HostError::Identity)?;
    if !metadata.is_file() {
        return Err(HostError::Identity);
    }
    Ok(metadata.uid())
}

#[cfg(not(unix))]
fn executable_owner(_path: &Path) -> Result<u32, HostError> {
    Err(HostError::Identity)
}

#[cfg(all(test, unix))]
#[path = "path_tests.rs"]
mod tests;
