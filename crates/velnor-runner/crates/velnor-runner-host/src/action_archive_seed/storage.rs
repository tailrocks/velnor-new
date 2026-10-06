use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};

use super::{ActionArchiveIdentity, ActionArchiveSeedError};

const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 2 * 1024 * 1024;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

pub(super) fn create_directory(path: &Path, mode: u32) -> Result<(), ActionArchiveSeedError> {
    if path.exists() {
        verify_real_directory(path)?;
    } else {
        fs::create_dir(path).map_err(|_| ActionArchiveSeedError::Io)?;
    }
    set_mode(path, mode)
}

pub(super) fn unique_directory(
    parent: &Path,
    prefix: &str,
) -> Result<PathBuf, ActionArchiveSeedError> {
    for _ in 0..100 {
        let counter = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(".{prefix}-{}-{counter}", std::process::id()));
        match fs::create_dir(&path) {
            Ok(()) => {
                if let Err(error) = set_mode(&path, 0o700) {
                    if fs::remove_dir(&path).is_err() {
                        return Err(ActionArchiveSeedError::Io);
                    }
                    return Err(error);
                }
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(ActionArchiveSeedError::Io),
        }
    }
    Err(ActionArchiveSeedError::Io)
}

pub(super) fn copy_verified<R: Read>(
    mut source: R,
    path: &Path,
    identity: &ActionArchiveIdentity,
) -> Result<(), ActionArchiveSeedError> {
    let mut output = create_private_file(path)?;
    let mut hash = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let count = source
            .read(&mut buffer)
            .map_err(|_| ActionArchiveSeedError::Io)?;
        if count == 0 {
            break;
        }
        size = size
            .checked_add(u64::try_from(count).map_err(|_| ActionArchiveSeedError::SizeLimit)?)
            .ok_or(ActionArchiveSeedError::SizeLimit)?;
        if size > MAX_ARCHIVE_BYTES || size > identity.size {
            return Err(ActionArchiveSeedError::SizeLimit);
        }
        hash.update(&buffer[..count]);
        output
            .write_all(&buffer[..count])
            .map_err(|_| ActionArchiveSeedError::Io)?;
    }
    if size != identity.size || hash.finalize().as_slice() != identity.sha256 {
        return Err(ActionArchiveSeedError::DigestMismatch);
    }
    output.sync_all().map_err(|_| ActionArchiveSeedError::Io)
}

pub(super) fn verify_bytes(
    path: &Path,
    identity: &ActionArchiveIdentity,
) -> Result<(), ActionArchiveSeedError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| ActionArchiveSeedError::StoreIntegrity)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != identity.size {
        return Err(ActionArchiveSeedError::StoreIntegrity);
    }
    let mut file = File::open(path).map_err(|_| ActionArchiveSeedError::Io)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| ActionArchiveSeedError::Io)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    if hash.finalize().as_slice() != identity.sha256 {
        return Err(ActionArchiveSeedError::StoreIntegrity);
    }
    Ok(())
}

pub(super) fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, ActionArchiveSeedError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| ActionArchiveSeedError::StoreIntegrity)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_MANIFEST_BYTES as u64
    {
        return Err(ActionArchiveSeedError::StoreIntegrity);
    }
    let file = File::open(path).map_err(|_| ActionArchiveSeedError::Io)?;
    serde_json::from_reader(file).map_err(|_| ActionArchiveSeedError::Manifest)
}

pub(super) fn write_json<T: Serialize>(
    path: &Path,
    value: &T,
) -> Result<(), ActionArchiveSeedError> {
    let bytes = serde_json::to_vec(value).map_err(|_| ActionArchiveSeedError::Manifest)?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(ActionArchiveSeedError::SizeLimit);
    }
    let mut file = create_private_file(path)?;
    file.write_all(&bytes)
        .map_err(|_| ActionArchiveSeedError::Io)?;
    file.sync_all().map_err(|_| ActionArchiveSeedError::Io)?;
    set_mode(path, 0o444)
}

pub(super) fn verify_real_directory(path: &Path) -> Result<(), ActionArchiveSeedError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| ActionArchiveSeedError::StoreIntegrity)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ActionArchiveSeedError::StoreIntegrity);
    }
    Ok(())
}

pub(super) fn set_mode(path: &Path, mode: u32) -> Result<(), ActionArchiveSeedError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .map_err(|_| ActionArchiveSeedError::Io)?;
    }
    #[cfg(not(unix))]
    {
        let mut permissions = fs::metadata(path)
            .map_err(|_| ActionArchiveSeedError::Io)?
            .permissions();
        permissions.set_readonly(mode & 0o222 == 0);
        fs::set_permissions(path, permissions).map_err(|_| ActionArchiveSeedError::Io)?;
    }
    Ok(())
}

pub(super) fn sync_directory(path: &Path) -> Result<(), ActionArchiveSeedError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| ActionArchiveSeedError::Io)
}

pub(super) fn cleanup_dir(path: &Path) -> Result<(), ActionArchiveSeedError> {
    remove_readonly_tree(path)
}

pub(super) fn remove_readonly_tree(path: &Path) -> Result<(), ActionArchiveSeedError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            set_mode(path, 0o700)?;
            for entry in fs::read_dir(path).map_err(|_| ActionArchiveSeedError::Io)? {
                let child = entry.map_err(|_| ActionArchiveSeedError::Io)?.path();
                let metadata =
                    fs::symlink_metadata(&child).map_err(|_| ActionArchiveSeedError::Io)?;
                if metadata.file_type().is_symlink() {
                    return Err(ActionArchiveSeedError::StoreIntegrity);
                }
                if metadata.is_dir() {
                    remove_readonly_tree(&child)?;
                } else if metadata.is_file() {
                    fs::remove_file(&child).map_err(|_| ActionArchiveSeedError::Io)?;
                } else {
                    return Err(ActionArchiveSeedError::StoreIntegrity);
                }
            }
            fs::remove_dir(path).map_err(|_| ActionArchiveSeedError::Io)
        }
        Ok(_) => Err(ActionArchiveSeedError::StoreIntegrity),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ActionArchiveSeedError::Io),
    }
}

fn create_private_file(path: &Path) -> Result<File, ActionArchiveSeedError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(|_| ActionArchiveSeedError::Io)
}
