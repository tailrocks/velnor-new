use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rustix::fs::{
    FileType, Mode, OFlags, RenameFlags, fstat, fsync, openat, renameat_with, unlinkat,
};

use crate::error::HostError;

use super::super::super::{DIRECTORY_MODE, FILE_MODE, check_capture_state};
use super::super::projection::MAX_PROJECTION_BYTES;

const MAX_TEMP_NAME_ATTEMPTS: usize = 8;
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

pub(super) fn write_projection(
    scope: &OwnedFd,
    name: &str,
    bytes: &[u8],
    started: Instant,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<(), HostError> {
    check_capture_state(started, deadline, cancelled)?;
    if bytes.is_empty() || bytes.len() > MAX_PROJECTION_BYTES {
        return Err(HostError::Ownership);
    }
    let mut staged = create_staged_file(scope, name)?;
    check_capture_state(started, deadline, cancelled)?;
    staged.file.write_all(bytes).map_err(|_| HostError::Path)?;
    check_capture_state(started, deadline, cancelled)?;
    staged.file.sync_all().map_err(|_| HostError::Path)?;
    check_capture_state(started, deadline, cancelled)?;

    match renameat_with(scope, &staged.name, scope, name, RenameFlags::NOREPLACE) {
        Ok(()) => staged.published = true,
        Err(error) if error == rustix::io::Errno::EXIST => {
            verify_existing(scope, name, bytes, started, deadline, cancelled)?;
            return Ok(());
        }
        Err(_) => return Err(HostError::Path),
    }
    fsync(scope).map_err(|_| HostError::Path)?;
    check_capture_state(started, deadline, cancelled)
}

fn create_staged_file<'a>(
    scope: &'a OwnedFd,
    final_name: &str,
) -> Result<StagedFile<'a>, HostError> {
    for _ in 0..MAX_TEMP_NAME_ATTEMPTS {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| HostError::Path)?
            .as_nanos();
        let name = format!(".{final_name}.tmp-{}-{now:x}-{id:x}", std::process::id());
        match openat(
            scope,
            &name,
            OFlags::WRONLY
                | OFlags::CREATE
                | OFlags::EXCL
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK
                | OFlags::CLOEXEC,
            expected_mode(FILE_MODE),
        ) {
            Ok(file) => {
                return Ok(StagedFile {
                    scope,
                    name,
                    file: File::from(file),
                    published: false,
                });
            }
            Err(error) if error == rustix::io::Errno::EXIST => {}
            Err(_) => return Err(HostError::Path),
        }
    }
    Err(HostError::Path)
}

struct StagedFile<'a> {
    scope: &'a OwnedFd,
    name: String,
    file: File,
    published: bool,
}

impl Drop for StagedFile<'_> {
    fn drop(&mut self) {
        if self.published {
            return;
        }
        let Ok(named) = openat(
            self.scope,
            &self.name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        ) else {
            return;
        };
        let Ok(original_stat) = fstat(&self.file) else {
            return;
        };
        let Ok(named_stat) = fstat(&named) else {
            return;
        };
        if original_stat.st_dev == named_stat.st_dev && original_stat.st_ino == named_stat.st_ino {
            match unlinkat(self.scope, &self.name, rustix::fs::AtFlags::empty()) {
                Ok(()) | Err(_) => {}
            }
        }
    }
}

fn verify_existing(
    scope: &OwnedFd,
    name: &str,
    expected: &[u8],
    started: Instant,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<(), HostError> {
    check_capture_state(started, deadline, cancelled)?;
    let file = openat(
        scope,
        name,
        OFlags::RDWR | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| HostError::Path)?;
    let metadata = fstat(&file).map_err(|_| HostError::Path)?;
    if FileType::from_raw_mode(metadata.st_mode) != FileType::RegularFile
        || metadata.st_uid != rustix::process::geteuid().as_raw()
        || Mode::from_raw_mode(metadata.st_mode) & Mode::from_raw_mode(0o777)
            != expected_mode(FILE_MODE)
        || metadata.st_size < 0
        || usize::try_from(metadata.st_size).map_err(|_| HostError::Path)? > MAX_PROJECTION_BYTES
    {
        return Err(HostError::Path);
    }
    let mut file = File::from(file);
    let mut found = Vec::with_capacity(expected.len());
    (&mut file)
        .take(u64::try_from(MAX_PROJECTION_BYTES + 1).map_err(|_| HostError::Path)?)
        .read_to_end(&mut found)
        .map_err(|_| HostError::Path)?;
    check_capture_state(started, deadline, cancelled)?;
    if found != expected {
        return Err(HostError::Ownership);
    }
    file.sync_all().map_err(|_| HostError::Path)?;
    fsync(scope).map_err(|_| HostError::Path)?;
    check_capture_state(started, deadline, cancelled)
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

#[cfg(target_os = "linux")]
fn expected_mode(mode: u16) -> Mode {
    Mode::from_raw_mode(u32::from(mode))
}

#[cfg(not(target_os = "linux"))]
fn expected_mode(mode: u16) -> Mode {
    Mode::from_raw_mode(mode)
}
