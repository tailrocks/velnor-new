//! Descriptor-anchored Linux configuration and credential reads.

use std::fs::File;
use std::io::Read;
use std::os::fd::{AsFd, OwnedFd};
use std::path::{Component, Path};

use rustix::fs::{FileType, Mode, OFlags, fstat, open, openat};
use rustix::io::Errno;

use crate::HostError;

use super::{LINUX_CONFIG_DIR, LINUX_CONFIG_PATH};

const MAX_CONFIG_BYTES: usize = 64 * 1024;

fn directory_flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}

fn file_flags() -> OFlags {
    OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC
}

#[derive(Clone, Copy)]
enum ModePolicy {
    Exact(u32),
    Clear(u32),
}

#[derive(Clone, Copy)]
struct DirectoryPolicy {
    uid: u32,
    gid: Option<u32>,
    mode: ModePolicy,
}

#[derive(Clone, Copy)]
struct FilePolicy {
    uid: u32,
    gid: Option<u32>,
    mode: ModePolicy,
    min_size: u64,
    max_size: u64,
}

pub(super) fn read_linux_config(path: &Path, group_id: u32) -> Result<Option<Vec<u8>>, HostError> {
    if path != Path::new(LINUX_CONFIG_PATH) {
        return Err(HostError::Config);
    }
    let directory = open_trusted_directory(
        Path::new(LINUX_CONFIG_DIR),
        DirectoryPolicy {
            uid: 0,
            gid: Some(group_id),
            mode: ModePolicy::Exact(0o750),
        },
    )?;
    let policy = FilePolicy {
        uid: 0,
        gid: Some(group_id),
        mode: ModePolicy::Exact(0o640),
        min_size: 0,
        max_size: MAX_CONFIG_BYTES as u64,
    };
    let Some(mut file) = open_regular_file_at(&directory, "host.toml", policy)? else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    file.by_ref()
        .take((MAX_CONFIG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| HostError::Config)?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(HostError::Config);
    }
    let stat = fstat(&file).map_err(|_| HostError::Config)?;
    validate_file_stat(stat, policy)?;
    if u64::try_from(stat.st_size).map_err(|_| HostError::Config)? != bytes.len() as u64 {
        return Err(HostError::Config);
    }
    Ok(Some(bytes))
}

/// Open the systemd credential using a validated directory descriptor and a
/// no-follow, nonblocking leaf open. The caller owns secret zeroization.
///
/// # Errors
///
/// Returns [`HostError::Config`] if any directory component or credential
/// file does not match the protected systemd credential contract.
pub fn open_systemd_credential_file(
    directory: &Path,
    name: &str,
    owner_uid: u32,
    max_size: usize,
) -> Result<File, HostError> {
    if !matches!(name, "github-token" | "actions-read-token") || max_size == 0 {
        return Err(HostError::Config);
    }
    let directory_fd = open_trusted_directory(
        directory,
        DirectoryPolicy {
            uid: owner_uid,
            gid: None,
            mode: ModePolicy::Clear(0o077),
        },
    )?;
    let policy = FilePolicy {
        uid: owner_uid,
        gid: None,
        mode: ModePolicy::Clear(0o7077),
        min_size: 1,
        max_size: max_size as u64,
    };
    open_regular_file_at(&directory_fd, name, policy)?.ok_or(HostError::Config)
}

fn open_trusted_directory(path: &Path, policy: DirectoryPolicy) -> Result<OwnedFd, HostError> {
    if !path.is_absolute() {
        return Err(HostError::Config);
    }
    let mut directory =
        open("/", directory_flags(), Mode::empty()).map_err(|_| HostError::Config)?;
    validate_trusted_ancestor(fstat(&directory).map_err(|_| HostError::Config)?)?;
    let components: Vec<_> = path.components().collect();
    if components.first() != Some(&Component::RootDir) || components.len() < 2 {
        return Err(HostError::Config);
    }
    for (index, component) in components.iter().enumerate().skip(1) {
        let Component::Normal(name) = component else {
            return Err(HostError::Config);
        };
        directory = openat(&directory, *name, directory_flags(), Mode::empty())
            .map_err(|_| HostError::Config)?;
        let stat = fstat(&directory).map_err(|_| HostError::Config)?;
        if index + 1 == components.len() {
            validate_directory_stat(stat, policy)?;
        } else {
            validate_trusted_ancestor(stat)?;
        }
    }
    Ok(directory)
}

fn open_regular_file_at<Fd: AsFd>(
    directory: Fd,
    name: &str,
    policy: FilePolicy,
) -> Result<Option<File>, HostError> {
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(HostError::Config);
    }
    let descriptor = match openat(directory, name, file_flags(), Mode::empty()) {
        Ok(descriptor) => descriptor,
        Err(Errno::NOENT) => return Ok(None),
        Err(_) => return Err(HostError::Config),
    };
    let stat = fstat(&descriptor).map_err(|_| HostError::Config)?;
    validate_file_stat(stat, policy)?;
    Ok(Some(File::from(descriptor)))
}

fn validate_directory_stat(
    stat: rustix::fs::Stat,
    policy: DirectoryPolicy,
) -> Result<(), HostError> {
    if !FileType::from_raw_mode(stat.st_mode).is_dir()
        || !owner_matches(stat.st_uid, stat.st_gid, policy.uid, policy.gid)
        || !mode_matches(stat.st_mode, policy.mode)
    {
        return Err(HostError::Config);
    }
    Ok(())
}

fn validate_trusted_ancestor(stat: rustix::fs::Stat) -> Result<(), HostError> {
    let mode = stat.st_mode & 0o7777;
    let writable = mode & 0o022 != 0;
    let protected_sticky_directory = mode & 0o1000 != 0;
    if !FileType::from_raw_mode(stat.st_mode).is_dir()
        || stat.st_uid != 0
        || (writable && !protected_sticky_directory)
    {
        return Err(HostError::Config);
    }
    Ok(())
}

fn validate_file_stat(stat: rustix::fs::Stat, policy: FilePolicy) -> Result<(), HostError> {
    let size = u64::try_from(stat.st_size).map_err(|_| HostError::Config)?;
    if !FileType::from_raw_mode(stat.st_mode).is_file()
        || !owner_matches(stat.st_uid, stat.st_gid, policy.uid, policy.gid)
        || !mode_matches(stat.st_mode, policy.mode)
        || size < policy.min_size
        || size > policy.max_size
    {
        return Err(HostError::Config);
    }
    Ok(())
}

fn owner_matches(
    actual_user: u32,
    actual_group: u32,
    required_user: u32,
    required_group: Option<u32>,
) -> bool {
    actual_user == required_user && required_group.is_none_or(|required| actual_group == required)
}

fn mode_matches(raw_mode: u32, policy: ModePolicy) -> bool {
    let mode = raw_mode & 0o7777;
    match policy {
        ModePolicy::Exact(expected) => mode == expected,
        ModePolicy::Clear(mask) => mode & mask == 0,
    }
}

#[cfg(test)]
mod tests;
