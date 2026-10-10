use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::HostError;

use super::super::path::{MAX_HELPER_BYTES, VerifiedHelper};
use super::store;
use super::{SnapshotLease, validate_open_file, validate_path_file};

const SNAPSHOT_SUFFIX: &str = ".helper";
const LOCK_SUFFIX: &str = ".lock";
const PARTIAL_PREFIX: &str = ".partial-";

pub(super) fn open_image(path: &Path, owner: u32) -> Result<File, HostError> {
    validate_path_file(path, owner, 0o500, MAX_HELPER_BYTES)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| HostError::Path)?;
    validate_open_file(path, &file, owner, 0o500, MAX_HELPER_BYTES)?;
    Ok(file)
}

pub(super) fn open_lease(
    directory: &Path,
    digest: &str,
    expected_sha256: &[u8; 32],
    owner: u32,
) -> Result<SnapshotLease, HostError> {
    let lock_path = directory.join(format!("{digest}{LOCK_SUFFIX}"));
    let lock = store::open_snapshot_shared_lock(&lock_path, owner)?;
    let path = directory.join(format!("{digest}{SNAPSHOT_SUFFIX}"));
    let mut image = open_image(&path, owner)?;
    if hash_file(&mut image)? != *expected_sha256 {
        return Err(HostError::Identity);
    }
    Ok(SnapshotLease {
        path,
        image,
        _lock: lock,
    })
}

pub(super) fn create_snapshot(
    directory: &Path,
    digest: &str,
    source: &mut VerifiedHelper,
    expected_sha256: &[u8; 32],
    owner: u32,
) -> Result<(), HostError> {
    let lock_path = directory.join(format!("{digest}{LOCK_SUFFIX}"));
    let _snapshot_lock = store::create_snapshot_lock(&lock_path, owner)?;
    let partial = directory.join(format!("{PARTIAL_PREFIX}{}", uuid::Uuid::new_v4()));
    let mut output = create_partial(&partial, owner)?;
    copy_verified(source, &mut output, expected_sha256)?;
    output
        .set_permissions(fs::Permissions::from_mode(0o500))
        .map_err(|_| HostError::Path)?;
    output.sync_all().map_err(|_| HostError::Path)?;
    publish(directory, &partial, digest)?;
    Ok(())
}

fn create_partial(path: &Path, owner: u32) -> Result<File, HostError> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    options
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = options.open(path).map_err(|_| HostError::Path)?;
    validate_open_file(path, &file, owner, 0o600, MAX_HELPER_BYTES)?;
    Ok(file)
}

fn copy_verified(
    source: &mut VerifiedHelper,
    output: &mut File,
    expected_sha256: &[u8; 32],
) -> Result<(), HostError> {
    source
        .file
        .seek(SeekFrom::Start(0))
        .map_err(|_| HostError::Identity)?;
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = source
            .file
            .read(&mut buffer)
            .map_err(|_| HostError::Identity)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(read).map_err(|_| HostError::Identity)?)
            .filter(|value| *value <= MAX_HELPER_BYTES)
            .ok_or(HostError::Identity)?;
        digest.update(&buffer[..read]);
        output
            .write_all(&buffer[..read])
            .map_err(|_| HostError::Path)?;
    }
    if total != source.length || digest.finalize().as_slice() != expected_sha256 {
        return Err(HostError::Identity);
    }
    output.sync_all().map_err(|_| HostError::Path)
}

fn publish(directory: &Path, partial: &Path, digest: &str) -> Result<(), HostError> {
    let ready = directory.join(format!("{digest}{SNAPSHOT_SUFFIX}"));
    match fs::symlink_metadata(&ready) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Ok(_) | Err(_) => return Err(HostError::Path),
    }
    fs::rename(partial, ready).map_err(|_| HostError::Path)?;
    File::open(directory)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| HostError::Path)
}

pub(super) fn hash_file(file: &mut File) -> Result<[u8; 32], HostError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| HostError::Identity)?;
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|_| HostError::Identity)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(read).map_err(|_| HostError::Identity)?)
            .filter(|value| *value <= MAX_HELPER_BYTES)
            .ok_or(HostError::Identity)?;
        digest.update(&buffer[..read]);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| HostError::Identity)?;
    Ok(digest.finalize().into())
}

pub(super) fn digest_name(digest: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut name = String::with_capacity(64);
    for byte in digest {
        name.push(char::from(HEX[usize::from(byte >> 4)]));
        name.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    name
}

pub(super) fn hex_digest(digest: &[u8; 32]) -> String {
    digest_name(digest)
}
