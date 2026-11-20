// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::time::Instant;

use rustix::fs::{self as rfs, AtFlags, Mode, OFlags};
const MAX_OCI_CANDIDATE_ARCHIVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;

use crate::error::HostError;

pub(super) struct SnapshotDirectory {
    pub(super) path: Option<tempfile::TempDir>,
    pub(super) handle: std::fs::File,
    pub(super) created: Vec<(String, std::fs::File)>,
}

impl SnapshotDirectory {
    pub(super) fn check(&self) -> Result<(), HostError> {
        let held = self.handle.metadata().map_err(|_| HostError::Ownership)?;
        let named =
            std::fs::symlink_metadata(self.path.as_ref().ok_or(HostError::Ownership)?.path())
                .map_err(|_| HostError::Ownership)?;
        if !held.is_dir()
            || !named.is_dir()
            || held.dev() != named.dev()
            || held.ino() != named.ino()
            || held.uid() != rustix::process::geteuid().as_raw()
            || held.mode() & 0o777 != 0o700
        {
            return Err(HostError::Ownership);
        }
        Ok(())
    }

    fn remove_owned(&mut self) -> Result<(), HostError> {
        self.check()?;
        let path = self.path.as_ref().ok_or(HostError::Ownership)?.path();
        for entry in std::fs::read_dir(path).map_err(|_| HostError::Ownership)? {
            let entry = entry.map_err(|_| HostError::Ownership)?;
            let name = entry.file_name();
            let (_, held) = self
                .created
                .iter()
                .find(|(created, _)| std::ffi::OsStr::new(created) == name)
                .ok_or(HostError::Ownership)?;
            let held = held.metadata().map_err(|_| HostError::Ownership)?;
            let named =
                std::fs::symlink_metadata(entry.path()).map_err(|_| HostError::Ownership)?;
            if !named.is_file() || held.dev() != named.dev() || held.ino() != named.ino() {
                return Err(HostError::Ownership);
            }
        }
        for (name, held) in &self.created {
            match rfs::statat(&self.handle, name.as_str(), AtFlags::SYMLINK_NOFOLLOW) {
                Ok(named) => {
                    let held = rfs::fstat(held).map_err(|_| HostError::Ownership)?;
                    if held.st_dev != named.st_dev || held.st_ino != named.st_ino {
                        return Err(HostError::Ownership);
                    }
                    rfs::unlinkat(&self.handle, name.as_str(), AtFlags::empty())
                        .map_err(|_| HostError::Ownership)?;
                }
                Err(_) => return Err(HostError::Ownership),
            }
        }
        self.path
            .take()
            .ok_or(HostError::Ownership)?
            .close()
            .map_err(|_| HostError::Ownership)
    }

    fn retain(&mut self) {
        if let Some(path) = self.path.take() {
            let retained = path.keep();
            eprintln!(
                "runner image qualification retained snapshots: {}",
                retained.display()
            );
        }
    }

    pub(super) fn close(mut self) -> Result<(), HostError> {
        let result = self.remove_owned();
        if result.is_err() {
            self.retain();
        }
        result
    }
}

impl Drop for SnapshotDirectory {
    fn drop(&mut self) {
        if self.path.is_some() && self.remove_owned().is_err() {
            self.retain();
        }
    }
}

pub(super) fn create_snapshot(
    source_path: &Path,
    directory: &mut SnapshotDirectory,
    name: &str,
    end: Instant,
) -> Result<std::fs::File, HostError> {
    create_bounded_snapshot(
        source_path,
        directory,
        name,
        end,
        MAX_OCI_CANDIDATE_ARCHIVE_BYTES,
    )
}

pub(super) fn create_bounded_snapshot(
    source_path: &Path,
    directory: &mut SnapshotDirectory,
    name: &str,
    end: Instant,
    limit: u64,
) -> Result<std::fs::File, HostError> {
    let source = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(source_path)
        .map_err(|_| HostError::Path)?;
    let source_metadata = source.metadata().map_err(|_| HostError::Path)?;
    if !source_metadata.is_file() || source_metadata.len() == 0 || source_metadata.len() > limit {
        return Err(HostError::Frame);
    }
    let mut output = std::fs::File::from(
        rfs::openat(
            &directory.handle,
            name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )
        .map_err(|_| HostError::Path)?,
    );
    directory.created.push((
        name.to_owned(),
        output.try_clone().map_err(|_| HostError::Ownership)?,
    ));
    let mut source = source;
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        if Instant::now() >= end {
            return Err(HostError::DockerTimeout);
        }
        let count = source.read(&mut buffer).map_err(|_| HostError::Path)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(count).map_err(|_| HostError::Frame)?)
            .ok_or(HostError::Frame)?;
        if total > limit {
            return Err(HostError::Frame);
        }
        output
            .write_all(&buffer[..count])
            .map_err(|_| HostError::Path)?;
    }
    if Instant::now() >= end {
        return Err(HostError::DockerTimeout);
    }
    if total != source_metadata.len() {
        return Err(HostError::Identity);
    }
    output.sync_all().map_err(|_| HostError::Path)?;
    output
        .set_permissions(std::fs::Permissions::from_mode(0o400))
        .map_err(|_| HostError::Path)?;
    let written = output.metadata().map_err(|_| HostError::Path)?;
    drop(output);
    let file = std::fs::File::from(
        rfs::openat(
            &directory.handle,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| HostError::Path)?,
    );
    let opened = file.metadata().map_err(|_| HostError::Path)?;
    if !opened.is_file()
        || opened.dev() != written.dev()
        || opened.ino() != written.ino()
        || opened.len() != total
    {
        return Err(HostError::Identity);
    }
    Ok(file)
}
