//! No-follow, no-clobber publication for private guest metadata.

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::error::HostError;

const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
const READ_ONLY_FILE_MODE: u32 = 0o444;

pub(super) fn publish(
    state_root: &Path,
    connection_id: &str,
    destination: &Path,
    bytes: &[u8],
) -> Result<(), HostError> {
    let directory = connection_directory(state_root, connection_id)?;
    if destination.parent() != Some(directory.as_path()) || bytes.is_empty() {
        return Err(HostError::Path);
    }
    if destination.exists() {
        return verify_file(destination, bytes);
    }
    let temporary = create_temporary(&directory, bytes)?;
    match fs::hard_link(&temporary.path, destination) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            drop(temporary);
            return verify_file(destination, bytes);
        }
        Err(_) => return Err(HostError::Path),
    }
    drop(temporary);
    sync_directory(&directory)?;
    verify_file(destination, bytes)
}

pub(super) fn verify_path(
    state_root: &Path,
    connection_id: &str,
    path: &Path,
    bytes: &[u8],
) -> Result<(), HostError> {
    let directory = connection_directory(state_root, connection_id)?;
    if path.parent() != Some(directory.as_path()) {
        return Err(HostError::ForbiddenMount);
    }
    verify_file(path, bytes)
}

fn connection_directory(state_root: &Path, connection_id: &str) -> Result<PathBuf, HostError> {
    if !state_root.is_absolute() || !safe_component(connection_id) {
        return Err(HostError::Path);
    }
    verify_state_directory(state_root)?;
    let image_root = state_root.join("guest-image");
    create_private_directory(&image_root)?;
    let connection = image_root.join(connection_id);
    create_private_directory(&connection)?;
    Ok(connection)
}

fn verify_state_directory(path: &Path) -> Result<(), HostError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| HostError::Path)?;
    if metadata.file_type().is_symlink()
        || !metadata.file_type().is_dir()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o022 != 0
    {
        return Err(HostError::Path);
    }
    Ok(())
}

fn create_private_directory(path: &Path) -> Result<(), HostError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => return validate_private_directory(&metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(HostError::Path),
    }
    let mut builder = DirBuilder::new();
    builder.mode(PRIVATE_DIRECTORY_MODE);
    match builder.create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(HostError::Path),
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| HostError::Path)?;
    if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
        fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_DIRECTORY_MODE))
            .map_err(|_| HostError::Path)?;
    }
    validate_private_directory(&fs::symlink_metadata(path).map_err(|_| HostError::Path)?)
}

fn validate_private_directory(metadata: &fs::Metadata) -> Result<(), HostError> {
    if metadata.file_type().is_symlink()
        || !metadata.file_type().is_dir()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o777 != PRIVATE_DIRECTORY_MODE
    {
        return Err(HostError::Path);
    }
    Ok(())
}

fn create_temporary(directory: &Path, bytes: &[u8]) -> Result<TemporaryFile, HostError> {
    for _attempt in 0..8 {
        let mut nonce = [0_u8; 16];
        getrandom::fill(&mut nonce).map_err(|_| HostError::Identity)?;
        let path = directory.join(format!(".tmp-{}-{}", std::process::id(), hex(&nonce)));
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        match options.open(&path) {
            Ok(file) => {
                let mut temporary = TemporaryFile { path, file };
                temporary
                    .file
                    .write_all(bytes)
                    .map_err(|_| HostError::Path)?;
                temporary.file.sync_all().map_err(|_| HostError::Path)?;
                temporary
                    .file
                    .set_permissions(fs::Permissions::from_mode(READ_ONLY_FILE_MODE))
                    .map_err(|_| HostError::Path)?;
                temporary.file.sync_all().map_err(|_| HostError::Path)?;
                return Ok(temporary);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(HostError::Path),
        }
    }
    Err(HostError::Path)
}

fn verify_file(path: &Path, expected: &[u8]) -> Result<(), HostError> {
    if expected.is_empty()
        || u64::try_from(expected.len()).map_or(true, |len| len > super::MAX_DOCUMENT_BYTES)
    {
        return Err(HostError::ForbiddenMount);
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    let mut file = options.open(path).map_err(|_| HostError::ForbiddenMount)?;
    verify_file_metadata(
        &file.metadata().map_err(|_| HostError::ForbiddenMount)?,
        expected,
    )?;
    let mut bytes = Vec::with_capacity(expected.len());
    (&mut file)
        .take(super::MAX_DOCUMENT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| HostError::ForbiddenMount)?;
    if bytes != expected {
        return Err(HostError::ForbiddenMount);
    }
    verify_file_metadata(
        &file.metadata().map_err(|_| HostError::ForbiddenMount)?,
        expected,
    )
}

fn verify_file_metadata(metadata: &fs::Metadata, expected: &[u8]) -> Result<(), HostError> {
    if !metadata.file_type().is_file()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o777 != READ_ONLY_FILE_MODE
        || metadata.nlink() != 1
        || metadata.len() != u64::try_from(expected.len()).map_err(|_| HostError::Path)?
    {
        return Err(HostError::ForbiddenMount);
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), HostError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| HostError::Path)
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(&byte))
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

struct TemporaryFile {
    path: PathBuf,
    file: File,
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        let _removed = fs::remove_file(&self.path);
    }
}
