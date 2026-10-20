use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use rustix::fs::{FileType, Mode, OFlags, fstat, fsync, mkdirat, openat};
use sha2::{Digest, Sha256};

use crate::error::HostError;

use super::{
    CAPTURE_DEADLINE, DIRECTORY_MODE, FILE_MODE, MAX_RECORD_BYTES, check_capture_state, hex,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct OperationKey {
    pub(super) journal_identity: String,
    pub(super) connection_id: String,
    pub(super) intent_id: i64,
}

pub(super) struct StageRequest {
    pub(super) state: OwnedFd,
    pub(super) parent: OwnedFd,
    pub(super) scope: OwnedFd,
    pub(super) key: OperationKey,
    pub(super) generation: i64,
    pub(super) role: String,
    pub(super) container_id: String,
    pub(super) bytes: Vec<u8>,
    pub(super) started: Instant,
}

pub(super) fn write_request(
    request: StageRequest,
    cancelled: &AtomicBool,
) -> Result<(), HostError> {
    let StageRequest {
        state,
        parent,
        scope,
        key,
        generation,
        role,
        container_id,
        bytes,
        started,
    } = request;
    validate_request(&key, generation, &role, &container_id, &bytes)?;
    let deadline = started
        .checked_add(CAPTURE_DEADLINE)
        .ok_or(HostError::DockerTimeout)?;
    check_capture_state(started, deadline, cancelled)?;
    validate_private_directory(&state)?;
    validate_private_directory(&parent)?;
    validate_private_directory(&scope)?;
    let container_hash = Sha256::digest(container_id.as_bytes());
    let name = format!(
        "i{}-g{}-{}-{}.json",
        key.intent_id,
        generation,
        role,
        hex(&container_hash)
    );
    let file = match openat(
        &scope,
        &name,
        OFlags::WRONLY
            | OFlags::CREATE
            | OFlags::EXCL
            | OFlags::NOFOLLOW
            | OFlags::NONBLOCK
            | OFlags::CLOEXEC,
        expected_mode(FILE_MODE),
    ) {
        Ok(file) => file,
        Err(error) if error == rustix::io::Errno::EXIST => {
            return verify_existing(&scope, &name, &bytes, started, deadline, cancelled);
        }
        Err(_) => return Err(HostError::Path),
    };
    check_capture_state(started, deadline, cancelled)?;
    let mut file = File::from(file);
    file.write_all(&bytes).map_err(|_| HostError::Path)?;
    check_capture_state(started, deadline, cancelled)?;
    file.sync_all().map_err(|_| HostError::Path)?;
    check_capture_state(started, deadline, cancelled)?;
    fsync(&scope).map_err(|_| HostError::Path)?;
    check_capture_state(started, deadline, cancelled)
}

pub(super) fn open_private_child(
    parent: &OwnedFd,
    name: &str,
    started: Instant,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<OwnedFd, HostError> {
    check_capture_state(started, deadline, cancelled)?;
    match mkdirat(parent, name, expected_mode(DIRECTORY_MODE)) {
        Ok(()) => {
            check_capture_state(started, deadline, cancelled)?;
            fsync(parent).map_err(|_| HostError::Path)?;
            check_capture_state(started, deadline, cancelled)?;
        }
        Err(error) if error == rustix::io::Errno::EXIST => {
            check_capture_state(started, deadline, cancelled)?;
        }
        Err(_) => return Err(HostError::Path),
    }
    let directory = openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| HostError::Path)?;
    check_capture_state(started, deadline, cancelled)?;
    validate_private_directory(&directory)?;
    check_capture_state(started, deadline, cancelled)?;
    Ok(directory)
}

fn validate_request(
    key: &OperationKey,
    generation: i64,
    role: &str,
    container_id: &str,
    bytes: &[u8],
) -> Result<(), HostError> {
    if key.journal_identity.is_empty()
        || key.connection_id.is_empty()
        || matches!(key.connection_id.as_str(), "." | "..")
        || key.intent_id <= 0
        || generation <= 0
        || !matches!(role, "runner" | "dind")
        || container_id.is_empty()
        || bytes.is_empty()
        || bytes.len() > MAX_RECORD_BYTES
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

pub(super) fn validate_private_directory(directory: &OwnedFd) -> Result<(), HostError> {
    let metadata = fstat(directory).map_err(|_| HostError::Path)?;
    if FileType::from_raw_mode(metadata.st_mode) != FileType::Directory
        || metadata.st_uid != rustix::process::geteuid().as_raw()
        || Mode::from_raw_mode(metadata.st_mode) & Mode::from_raw_mode(0o777)
            != expected_mode(DIRECTORY_MODE)
    {
        return Err(HostError::Path);
    }
    Ok(())
}

fn verify_existing(
    directory: &OwnedFd,
    name: &str,
    expected: &[u8],
    started: Instant,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<(), HostError> {
    check_capture_state(started, deadline, cancelled)?;
    let file = openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| HostError::Path)?;
    check_capture_state(started, deadline, cancelled)?;
    let stat = fstat(&file).map_err(|_| HostError::Path)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
        || stat.st_uid != rustix::process::geteuid().as_raw()
        || Mode::from_raw_mode(stat.st_mode) & Mode::from_raw_mode(0o777)
            != expected_mode(FILE_MODE)
        || stat.st_size < 0
        || usize::try_from(stat.st_size).map_err(|_| HostError::Path)? > MAX_RECORD_BYTES
    {
        return Err(HostError::Path);
    }
    let mut bytes = Vec::with_capacity(MAX_RECORD_BYTES);
    File::from(file)
        .take(u64::try_from(MAX_RECORD_BYTES + 1).map_err(|_| HostError::Path)?)
        .read_to_end(&mut bytes)
        .map_err(|_| HostError::Path)?;
    check_capture_state(started, deadline, cancelled)?;
    if bytes.as_slice() != expected {
        return Err(HostError::Ownership);
    }
    let file = openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| HostError::Path)?;
    check_capture_state(started, deadline, cancelled)?;
    File::from(file).sync_all().map_err(|_| HostError::Path)?;
    check_capture_state(started, deadline, cancelled)?;
    fsync(directory).map_err(|_| HostError::Path)?;
    check_capture_state(started, deadline, cancelled)
}

#[cfg(target_os = "linux")]
fn expected_mode(mode: u16) -> Mode {
    Mode::from_raw_mode(u32::from(mode))
}

#[cfg(not(target_os = "linux"))]
fn expected_mode(mode: u16) -> Mode {
    Mode::from_raw_mode(mode)
}
