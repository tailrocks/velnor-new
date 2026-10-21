use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use crate::error::HostError;
use rustix::fs::{FileType, Mode, OFlags, fstat, fsync, openat, renameat};

use super::{
    CapturePhase, CaptureState, DIRECTORY_MODE, FILE_MODE, STATE_BYTES, TEMP_FILE,
    check_capture_state, valid_id,
};

pub(super) fn write_state(
    scope: &OwnedFd,
    name: &str,
    bytes: &[u8],
    started: Instant,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<(), HostError> {
    validate_directory(scope)?;
    validate_transition(scope, name, bytes)?;
    check_capture_state(started, deadline, cancelled)?;
    write_or_recover_temp(scope, bytes, started, deadline, cancelled)?;
    check_capture_state(started, deadline, cancelled)?;
    validate_existing_state(scope, name)?;
    renameat(scope, TEMP_FILE, scope, name).map_err(|_| HostError::Path)?;
    fsync(scope).map_err(|_| HostError::Path)?;
    check_capture_state(started, deadline, cancelled)
}

fn validate_transition(scope: &OwnedFd, name: &str, bytes: &[u8]) -> Result<(), HostError> {
    let next: CaptureState = serde_json::from_slice(bytes).map_err(|_| HostError::Ownership)?;
    next.validate(&next.binding)?;
    let Some(current_bytes) = read_private(scope, name, STATE_BYTES)? else {
        return if next.phase == CapturePhase::Prepared && next.attempt == 1 {
            Ok(())
        } else {
            Err(HostError::Ownership)
        };
    };
    if current_bytes == bytes {
        return Ok(());
    }
    let current: CaptureState =
        serde_json::from_slice(&current_bytes).map_err(|_| HostError::Ownership)?;
    current.validate(&current.binding)?;
    if current.binding != next.binding {
        return Err(HostError::Ownership);
    }
    if allowed_transition(&current, &next) {
        Ok(())
    } else {
        Err(HostError::Ownership)
    }
}

fn allowed_transition(current: &CaptureState, next: &CaptureState) -> bool {
    let same_exec = current.exec_id == next.exec_id;
    match (current.phase, next.phase) {
        (CapturePhase::Prepared, CapturePhase::Prepared) => {
            current.attempt == next.attempt
                && current.exec_id.is_none()
                && current.create_requests.checked_add(1) == Some(next.create_requests)
                && current.start_requests == next.start_requests
        }
        (CapturePhase::Prepared, CapturePhase::Created) => {
            current.attempt == next.attempt
                && current.create_requests > 0
                && current.create_requests == next.create_requests
                && current.exec_id.is_none()
                && next.exec_id.as_deref().is_some_and(valid_id)
        }
        (CapturePhase::Created, CapturePhase::StartRequested) => {
            current.attempt == next.attempt
                && same_exec
                && current.create_requests == next.create_requests
                && next.start_requests == 1
        }
        (CapturePhase::StartRequested, CapturePhase::StartRequested) => {
            current.attempt == next.attempt
                && same_exec
                && current.create_requests == next.create_requests
                && current.start_requests.checked_add(1) == Some(next.start_requests)
        }
        (CapturePhase::StartRequested, CapturePhase::Terminal) => {
            current.attempt == next.attempt
                && same_exec
                && current.create_requests == next.create_requests
                && current.start_requests == next.start_requests
                && next.exit_code.is_some()
        }
        (CapturePhase::Terminal, CapturePhase::ProjectionPending) => {
            current.attempt == next.attempt
                && same_exec
                && current.create_requests == next.create_requests
                && current.start_requests == next.start_requests
                && next.exit_code == current.exit_code
                && next.projection_sha256.is_some()
                && next.projection_bytes.is_some()
                && next.has_trace.is_some()
        }
        (CapturePhase::ProjectionPending, CapturePhase::Complete | CapturePhase::Unavailable) => {
            current.attempt == next.attempt
                && same_exec
                && current.create_requests == next.create_requests
                && current.start_requests == next.start_requests
                && current.exit_code == next.exit_code
                && current.projection_sha256 == next.projection_sha256
                && current.projection_bytes == next.projection_bytes
                && current.has_trace == next.has_trace
        }
        (CapturePhase::Terminal, CapturePhase::Prepared) => {
            current.attempt.checked_add(1) == Some(next.attempt)
                && next.exec_id.is_none()
                && next.create_requests == 0
                && next.start_requests == 0
        }
        (CapturePhase::ProjectionPending, CapturePhase::Prepared) => {
            current.attempt.checked_add(1) == Some(next.attempt)
                && next.exec_id.is_none()
                && next.create_requests == 0
                && next.start_requests == 0
        }
        _ => false,
    }
}

fn write_or_recover_temp(
    scope: &OwnedFd,
    bytes: &[u8],
    started: Instant,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<(), HostError> {
    match openat(
        scope,
        TEMP_FILE,
        OFlags::RDWR | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    ) {
        Ok(file) => {
            validate_file(&file, STATE_BYTES)?;
            let mut file = File::from(file);
            let mut staged = Vec::new();
            let mut reader = (&mut file).take((STATE_BYTES + 1) as u64);
            reader
                .read_to_end(&mut staged)
                .map_err(|_| HostError::Path)?;
            drop(reader);
            if staged != bytes {
                return Err(HostError::Ownership);
            }
            check_capture_state(started, deadline, cancelled)?;
            file.sync_all().map_err(|_| HostError::Path)
        }
        Err(error) if error == rustix::io::Errno::NOENT => {
            let file = openat(
                scope,
                TEMP_FILE,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                expected_mode(FILE_MODE),
            )
            .map_err(|_| HostError::Path)?;
            let mut file = File::from(file);
            file.write_all(bytes).map_err(|_| HostError::Path)?;
            check_capture_state(started, deadline, cancelled)?;
            file.sync_all().map_err(|_| HostError::Path)
        }
        Err(_) => Err(HostError::Path),
    }
}

fn validate_existing_state(scope: &OwnedFd, name: &str) -> Result<(), HostError> {
    match openat(
        scope,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    ) {
        Ok(file) => validate_file(&file, STATE_BYTES),
        Err(error) if error == rustix::io::Errno::NOENT => Ok(()),
        Err(_) => Err(HostError::Path),
    }
}

pub(super) fn read_private(
    scope: &OwnedFd,
    name: &str,
    max: usize,
) -> Result<Option<Vec<u8>>, HostError> {
    let file = match openat(
        scope,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    ) {
        Ok(file) => file,
        Err(error) if error == rustix::io::Errno::NOENT => return Ok(None),
        Err(_) => return Err(HostError::Path),
    };
    validate_file(&file, max)?;
    let mut bytes = Vec::new();
    File::from(file)
        .take(u64::try_from(max + 1).map_err(|_| HostError::Path)?)
        .read_to_end(&mut bytes)
        .map_err(|_| HostError::Path)?;
    if bytes.is_empty() || bytes.len() > max {
        return Err(HostError::Ownership);
    }
    Ok(Some(bytes))
}

fn validate_file(file: &OwnedFd, max: usize) -> Result<(), HostError> {
    let metadata = fstat(file).map_err(|_| HostError::Path)?;
    if FileType::from_raw_mode(metadata.st_mode) != FileType::RegularFile
        || metadata.st_uid != rustix::process::geteuid().as_raw()
        || Mode::from_raw_mode(metadata.st_mode) & Mode::from_raw_mode(0o777)
            != expected_mode(FILE_MODE)
        || metadata.st_size < 0
        || usize::try_from(metadata.st_size).map_err(|_| HostError::Path)? > max
    {
        return Err(HostError::Path);
    }
    Ok(())
}

fn validate_directory(directory: &OwnedFd) -> Result<(), HostError> {
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

#[cfg(target_os = "linux")]
fn expected_mode(mode: u16) -> Mode {
    Mode::from_raw_mode(u32::from(mode))
}

#[cfg(not(target_os = "linux"))]
fn expected_mode(mode: u16) -> Mode {
    Mode::from_raw_mode(mode)
}
