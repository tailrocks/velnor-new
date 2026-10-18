use super::StageError;
use super::checked_stat_u64;
use super::context::StageContext;
use rustix::fs::{AtFlags, Mode, OFlags, mkdirat, open, openat, statat};
use rustix::io::Errno;
use std::cmp::Ordering;
use std::ffi::CString;
use std::os::fd::{AsFd, OwnedFd};
use std::path::{Component, Path};

const PRIVATE_DIRECTORY_MODE: Mode = Mode::from_bits_retain(0o700);
const ROOT_UID: u32 = 0;

/// Keeps every directory descriptor from the runner temporary root to the payload parent.
pub(super) struct StageParent {
    directories: Vec<OwnedFd>,
    names: Vec<CString>,
    payload_name: CString,
    runner_temp_index: usize,
    runner_temp_identity: (u64, u64),
    filesystem_device: u64,
    filesystem_mount_id: Option<u64>,
}

impl StageParent {
    pub(super) fn file_directory(&self) -> Result<&OwnedFd, StageError> {
        self.directories
            .last()
            .ok_or("cache_stage_parent_open_failed")
    }

    pub(super) fn payload_name(&self) -> &CString {
        &self.payload_name
    }

    pub(super) fn revalidate(&self) -> Result<(), StageError> {
        for (index, name) in self.names.iter().enumerate() {
            let parent = self
                .directories
                .get(index)
                .ok_or("cache_stage_parent_open_failed")?;
            let child = self
                .directories
                .get(index + 1)
                .ok_or("cache_stage_parent_open_failed")?;
            let path_stat = statat(parent, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)
                .map_err(|_| "cache_stage_path_changed")?;
            let descriptor_stat =
                rustix::fs::fstat(child.as_fd()).map_err(|_| "cache_stage_parent_stat_failed")?;
            if !same_directory(&path_stat, &descriptor_stat) {
                return Err("cache_stage_path_changed");
            }
            match (index + 1).cmp(&self.runner_temp_index) {
                Ordering::Equal => {
                    if directory_identity(&descriptor_stat)? != self.runner_temp_identity {
                        return Err("cache_stage_runner_temp_changed");
                    }
                    validate_private_directory(child)?;
                }
                Ordering::Greater => validate_stage_directory(
                    child,
                    self.filesystem_device,
                    self.filesystem_mount_id,
                )?,
                Ordering::Less => validate_ancestor(child)?,
            }
        }
        Ok(())
    }
}

/// Opens or safely creates the fixed quota path while retaining its directory chain.
pub(super) fn open_stage_parent(
    context: &StageContext,
    create: bool,
) -> Result<StageParent, StageError> {
    let mut directories = vec![open_root()?];
    let mut names = Vec::new();
    let mut runner_temp_index = None;
    let mut runner_components = Vec::new();
    for component in context.runner_temp().components() {
        match component {
            Component::Normal(value) => runner_components.push(value),
            Component::RootDir => {}
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                return Err("cache_stage_path_invalid");
            }
        }
    }
    if runner_components.is_empty() {
        return Err("cache_stage_path_invalid");
    }
    for (index, component) in runner_components.iter().enumerate() {
        let name = component_name(component)?;
        let current_parent = last_directory(&directories)?;
        let child = open_child(current_parent, &name)?;
        validate_ancestor(&child)?;
        let descriptor_stat =
            rustix::fs::fstat(child.as_fd()).map_err(|_| "cache_stage_parent_stat_failed")?;
        let expected = context.runner_temp_identity();
        if index + 1 == runner_components.len() {
            if directory_identity(&descriptor_stat)? != expected {
                return Err("cache_stage_runner_temp_changed");
            }
            validate_private_directory(&child)?;
            runner_temp_index = Some(directories.len());
        }
        verify_named_child(current_parent, &name, &child)?;
        names.push(name);
        directories.push(child);
    }
    let runner_temp_index = runner_temp_index.ok_or("cache_stage_path_invalid")?;
    let runner_temp = directories
        .get(runner_temp_index)
        .ok_or("cache_stage_parent_open_failed")?;
    let runner_temp_stat =
        rustix::fs::fstat(runner_temp.as_fd()).map_err(|_| "cache_stage_parent_stat_failed")?;
    let filesystem_device =
        checked_stat_u64(runner_temp_stat.st_dev, "cache_stage_parent_stat_failed")?;
    #[cfg(target_os = "linux")]
    let filesystem_mount_id = Some(directory_mount_id(runner_temp)?);
    #[cfg(not(target_os = "linux"))]
    let filesystem_mount_id = None;

    for component in context.stage_directory_components()? {
        let name = CString::new(component).map_err(|_| "cache_stage_path_invalid")?;
        let parent = last_directory(&directories)?;
        if create {
            match mkdirat(parent, name.as_c_str(), PRIVATE_DIRECTORY_MODE) {
                Ok(()) | Err(Errno::EXIST) => {}
                Err(_) => return Err("cache_stage_directory_create_failed"),
            }
        }
        let child = open_child(parent, &name)?;
        validate_stage_directory(&child, filesystem_device, filesystem_mount_id)?;
        verify_named_child(parent, &name, &child)?;
        names.push(name);
        directories.push(child);
    }
    let payload_name = CString::new("payload").map_err(|_| "cache_stage_path_invalid")?;
    let parent = StageParent {
        directories,
        names,
        payload_name,
        runner_temp_index,
        runner_temp_identity: context.runner_temp_identity(),
        filesystem_device,
        filesystem_mount_id,
    };
    parent.revalidate()?;
    Ok(parent)
}

pub(super) fn open_directory_path(path: &Path) -> Result<OwnedFd, StageError> {
    if !path.is_absolute() {
        return Err("cache_stage_path_invalid");
    }
    let mut fd = open_root()?;
    validate_ancestor(&fd)?;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(value) => {
                let name = component_name(value)?;
                fd = open_child(&fd, &name)?;
                validate_ancestor(&fd)?;
            }
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                return Err("cache_stage_path_invalid");
            }
        }
    }
    Ok(fd)
}

pub(super) fn validate_parent(fd: &OwnedFd) -> Result<(), StageError> {
    validate_private_directory(fd)
}

pub(super) fn verify_payload_entry(
    parent: &StageParent,
    file: &rustix::fs::Stat,
) -> Result<bool, StageError> {
    parent.revalidate()?;
    let path = statat(
        parent.file_directory()?,
        parent.payload_name().as_c_str(),
        AtFlags::SYMLINK_NOFOLLOW,
    )
    .map_err(|_| "cache_stage_path_stat_failed")?;
    Ok(path.st_dev == file.st_dev && path.st_ino == file.st_ino && path.st_mode == file.st_mode)
}

fn open_root() -> Result<OwnedFd, StageError> {
    open(
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "cache_stage_parent_open_failed")
}

fn last_directory(directories: &[OwnedFd]) -> Result<&OwnedFd, StageError> {
    directories.last().ok_or("cache_stage_parent_open_failed")
}

fn open_child(parent: &OwnedFd, name: &CString) -> Result<OwnedFd, StageError> {
    openat(
        parent,
        name.as_c_str(),
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "cache_stage_parent_open_failed")
}

fn component_name(component: &std::ffi::OsStr) -> Result<CString, StageError> {
    CString::new(component.to_str().ok_or("cache_stage_path_invalid")?)
        .map_err(|_| "cache_stage_path_invalid")
}

fn verify_named_child(parent: &OwnedFd, name: &CString, child: &OwnedFd) -> Result<(), StageError> {
    let path_stat = statat(parent, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|_| "cache_stage_path_stat_failed")?;
    let descriptor_stat =
        rustix::fs::fstat(child.as_fd()).map_err(|_| "cache_stage_parent_stat_failed")?;
    if !same_directory(&path_stat, &descriptor_stat) {
        return Err("cache_stage_path_changed");
    }
    Ok(())
}

fn same_directory(left: &rustix::fs::Stat, right: &rustix::fs::Stat) -> bool {
    left.st_dev == right.st_dev
        && left.st_ino == right.st_ino
        && left.st_mode == right.st_mode
        && left.st_uid == right.st_uid
        && rustix::fs::FileType::from_raw_mode(left.st_mode) == rustix::fs::FileType::Directory
}

fn directory_identity(stat: &rustix::fs::Stat) -> Result<(u64, u64), StageError> {
    Ok((
        checked_stat_u64(stat.st_dev, "cache_stage_parent_stat_failed")?,
        checked_stat_u64(stat.st_ino, "cache_stage_parent_stat_failed")?,
    ))
}

fn validate_stage_directory(
    fd: &OwnedFd,
    filesystem_device: u64,
    filesystem_mount_id: Option<u64>,
) -> Result<(), StageError> {
    let stat = rustix::fs::fstat(fd.as_fd()).map_err(|_| "cache_stage_parent_stat_failed")?;
    let device = checked_stat_u64(stat.st_dev, "cache_stage_parent_stat_failed")?;
    #[cfg(target_os = "linux")]
    let mount_id = Some(directory_mount_id(fd)?);
    #[cfg(not(target_os = "linux"))]
    let mount_id = None;
    if device != filesystem_device || mount_id != filesystem_mount_id {
        return Err("cache_stage_filesystem_changed");
    }
    validate_private_directory(fd)
}

#[cfg(target_os = "linux")]
fn directory_mount_id(fd: &OwnedFd) -> Result<u64, StageError> {
    use rustix::fs::{AtFlags, StatxFlags, statx};

    let stat = statx(fd, "", AtFlags::EMPTY_PATH, StatxFlags::MNT_ID)
        .map_err(|_| "cache_stage_mount_identity_unavailable")?;
    if StatxFlags::from_bits_retain(stat.stx_mask).contains(StatxFlags::MNT_ID) {
        Ok(stat.stx_mnt_id)
    } else {
        Err("cache_stage_mount_identity_unavailable")
    }
}

fn validate_private_directory(fd: &OwnedFd) -> Result<(), StageError> {
    let stat = rustix::fs::fstat(fd.as_fd()).map_err(|_| "cache_stage_parent_stat_failed")?;
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::Directory
        || stat.st_uid != crate::process_owner::effective_uid()
        || stat.st_mode & 0o7022 != 0
    {
        return Err("cache_stage_parent_owner_or_mode_invalid");
    }
    Ok(())
}

fn validate_ancestor(fd: &OwnedFd) -> Result<(), StageError> {
    let stat = rustix::fs::fstat(fd.as_fd()).map_err(|_| "cache_stage_parent_stat_failed")?;
    let mode = stat.st_mode & 0o7777;
    let writable_by_others = mode & 0o0022 != 0;
    let protected_by_sticky_bit = mode & 0o1000 != 0;
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::Directory
        || (stat.st_uid != ROOT_UID && stat.st_uid != crate::process_owner::effective_uid())
        || mode & 0o6000 != 0
        || (writable_by_others && !protected_by_sticky_bit)
    {
        return Err("cache_stage_ancestor_owner_or_mode_invalid");
    }
    Ok(())
}
