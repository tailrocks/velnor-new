// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

#[cfg(unix)]
use std::io::{Read, Write};

use crate::error::HostError;

#[cfg(unix)]
pub(super) fn read_bounded_regular(path: &Path, limit: usize) -> Result<Vec<u8>, HostError> {
    use rustix::fs::{Mode, OFlags, openat};
    use std::os::unix::fs::MetadataExt;

    let parent = held_parent(path)?;
    let before = std::fs::symlink_metadata(path).map_err(|_| HostError::Path)?;
    if !before.is_file() || before.len() > to_u64(limit)? {
        return Err(HostError::Frame);
    }

    let file = std::fs::File::from(
        openat(
            &parent,
            path.file_name().ok_or(HostError::Path)?,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| HostError::Path)?,
    );
    let opened = file.metadata().map_err(|_| HostError::Path)?;
    if !opened.is_file()
        || opened.dev() != before.dev()
        || opened.ino() != before.ino()
        || opened.len() > to_u64(limit)?
    {
        return Err(HostError::Frame);
    }
    let mut bytes = Vec::with_capacity(limit);
    (&file)
        .take(to_u64(limit.saturating_add(1))?)
        .read_to_end(&mut bytes)
        .map_err(|_| HostError::Path)?;
    let after = std::fs::symlink_metadata(path).map_err(|_| HostError::Path)?;
    let final_opened = file.metadata().map_err(|_| HostError::Path)?;
    if bytes.len() > limit
        || !after.is_file()
        || after.dev() != opened.dev()
        || after.ino() != opened.ino()
        || final_opened.dev() != opened.dev()
        || final_opened.ino() != opened.ino()
        || final_opened.len() != opened.len()
    {
        return Err(HostError::Frame);
    }
    Ok(bytes)
}

#[cfg(not(unix))]
pub(super) fn read_bounded_regular(_path: &Path, _limit: usize) -> Result<Vec<u8>, HostError> {
    Err(HostError::Identity)
}

#[cfg(unix)]
pub(super) fn create_new_regular(path: &Path, bytes: &[u8]) -> Result<(), HostError> {
    use rustix::fs::{Mode, OFlags, openat};

    let parent = held_parent(path)?;
    let mut file = std::fs::File::from(
        openat(
            &parent,
            path.file_name().ok_or(HostError::Path)?,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o666),
        )
        .map_err(|_| HostError::Path)?,
    );
    file.write_all(bytes).map_err(|_| HostError::Path)?;
    file.sync_all().map_err(|_| HostError::Path)
}

#[cfg(not(unix))]
pub(super) fn create_new_regular(_path: &Path, _bytes: &[u8]) -> Result<(), HostError> {
    Err(HostError::Identity)
}

#[cfg(unix)]
fn held_parent(path: &Path) -> Result<std::fs::File, HostError> {
    use rustix::fs::{Mode, OFlags, open};
    use std::os::unix::fs::MetadataExt;

    let parent_path = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let before = std::fs::symlink_metadata(parent_path).map_err(|_| HostError::Path)?;
    if !before.is_dir() {
        return Err(HostError::Path);
    }
    let parent = std::fs::File::from(
        open(
            parent_path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| HostError::Path)?,
    );
    let opened = parent.metadata().map_err(|_| HostError::Path)?;
    if !opened.is_dir() || opened.dev() != before.dev() || opened.ino() != before.ino() {
        return Err(HostError::Path);
    }
    Ok(parent)
}

#[cfg(unix)]
fn to_u64(value: usize) -> Result<u64, HostError> {
    u64::try_from(value).map_err(|_| HostError::Frame)
}
