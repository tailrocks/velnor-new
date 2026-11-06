// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::fd::AsFd;
use std::path::Path;

use rustix::fs::{self, Mode, Stat};

use crate::OrchestratorError;
use crate::generation_filesystem::{FilesystemMetadata, FilesystemObjectType, MountIdentity, io};

#[cfg(target_os = "linux")]
pub(super) fn linux_mount_from_fd(
    fd: impl AsFd,
    path: &Path,
) -> Result<MountIdentity, OrchestratorError> {
    use rustix::fs::{AtFlags, StatxFlags, statx};

    let status =
        statx(fd, "", AtFlags::EMPTY_PATH, StatxFlags::MNT_ID).map_err(|error| io(path, &error))?;
    if !StatxFlags::from_bits_retain(status.stx_mask).contains(StatxFlags::MNT_ID) {
        return Err(OrchestratorError::unsupported(
            "in_place_generation",
            "linux_statx_mount_id_unavailable",
        ));
    }
    Ok(MountIdentity::Linux {
        mount_id: status.stx_mnt_id,
    })
}

#[cfg(target_os = "macos")]
pub(super) fn macos_mount_from_fd(
    fd: impl AsFd,
    path: &Path,
) -> Result<MountIdentity, OrchestratorError> {
    let fd = fd.as_fd();
    let status = fs::fstatfs(fd).map_err(|error| io(path, &error))?;
    let filesystem_id = fs::fstatvfs(fd).map_err(|error| io(path, &error))?.f_fsid;
    let mount_point = status
        .f_mntonname
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|byte| byte.cast_unsigned())
        .collect();
    Ok(MountIdentity::Macos {
        mount_point,
        filesystem_id,
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn metadata_from_stat(stat: &Stat, mount: MountIdentity) -> FilesystemMetadata {
    FilesystemMetadata {
        #[cfg(target_os = "macos")]
        device: u64::from(stat.st_dev.cast_unsigned()),
        #[cfg(target_os = "linux")]
        device: stat.st_dev,
        inode: stat.st_ino,
        owner: stat.st_uid,
        #[cfg(target_os = "macos")]
        mode: u32::from(stat.st_mode) & 0o7777,
        #[cfg(target_os = "linux")]
        mode: stat.st_mode & 0o7777,
        #[cfg(target_os = "macos")]
        link_count: u64::from(stat.st_nlink),
        #[cfg(target_os = "linux")]
        link_count: stat.st_nlink,
        object_type: object_type(stat.st_mode),
        mount,
    }
}

#[cfg(target_os = "macos")]
pub(super) fn mode_from_permissions(mode: u32) -> Result<Mode, std::io::Error> {
    u16::try_from(mode)
        .map(Mode::from_raw_mode)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))
}

#[cfg(target_os = "linux")]
pub(super) fn mode_from_permissions(mode: u32) -> Result<Mode, std::io::Error> {
    Ok(Mode::from_raw_mode(mode))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn object_type(mode: rustix::fs::RawMode) -> FilesystemObjectType {
    match rustix::fs::FileType::from_raw_mode(mode) {
        rustix::fs::FileType::Directory => FilesystemObjectType::Directory,
        rustix::fs::FileType::RegularFile => FilesystemObjectType::RegularFile,
        rustix::fs::FileType::Symlink => FilesystemObjectType::Symlink,
        _ => FilesystemObjectType::Other,
    }
}
