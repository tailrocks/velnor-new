//! This module measures cache use and available space in the runner work directory.

#[cfg(target_os = "linux")]
use super::RunnerWorkFilesystem;
use super::VerifiedCacheBudget;
#[cfg(target_os = "linux")]
use crate::qualification_cache_payload::transfer::payload;
#[cfg(target_os = "linux")]
use rustix::fs::{AtFlags, Dir, FileType, OFlags, Stat, openat, statat};
#[cfg(target_os = "linux")]
use std::collections::HashSet;
#[cfg(target_os = "linux")]
use std::ffi::CStr;

#[cfg(target_os = "linux")]
const MAX_ENTRIES: usize = 250_000;
#[cfg(target_os = "linux")]
const MAX_DEPTH: usize = 128;
#[cfg(target_os = "linux")]
const PATH_LIMIT: usize = 4_096;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::qualification_cache_payload) struct CacheTreeUsage {
    pub(in crate::qualification_cache_payload) bytes: u64,
    pub(in crate::qualification_cache_payload) inodes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::qualification_cache_payload) struct FreshCapacity {
    pub(in crate::qualification_cache_payload) available_bytes: u64,
    pub(in crate::qualification_cache_payload) available_inodes: Option<u64>,
    pub(in crate::qualification_cache_payload) fragment_bytes: u64,
}

pub(in crate::qualification_cache_payload) fn observe_cache_storage(
    budget: &VerifiedCacheBudget,
    work_anchor: &crate::qualification_cache_payload::transfer::source::SourceDirectory,
    live: Option<&crate::qualification_cache_payload::transfer::source::SourceDirectory>,
    stage: Option<&crate::qualification_cache_payload::transfer::source::SourceDirectory>,
) -> Result<(CacheTreeUsage, CacheTreeUsage, FreshCapacity), &'static str> {
    #[cfg(target_os = "linux")]
    {
        let expected = budget.runner_work();
        let fresh_capacity = filesystem_capacity(work_anchor.fd(), expected)?;
        let live_usage = match live {
            Some(directory) => {
                filesystem_capacity(directory.fd(), expected)?;
                measure_tree(directory.fd())?
            }
            None => CacheTreeUsage::default(),
        };
        let stage_usage = match stage {
            Some(directory) => {
                filesystem_capacity(directory.fd(), expected)?;
                measure_tree(directory.fd())?
            }
            None => CacheTreeUsage::default(),
        };
        Ok((live_usage, stage_usage, fresh_capacity))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (budget, work_anchor, live, stage);
        Err("cache_storage_filesystem_unavailable")
    }
}

#[cfg(target_os = "linux")]
fn filesystem_capacity(
    directory: &rustix::fd::OwnedFd,
    expected: RunnerWorkFilesystem,
) -> Result<FreshCapacity, &'static str> {
    use rustix::fs::{AtFlags, StatxFlags, fstatfs, fstatvfs, statx};

    let stat =
        rustix::fs::fstat(directory).map_err(|_| "cache_storage_filesystem_observation_failed")?;
    let device = stat.st_dev;
    let observed = statx(
        directory,
        "",
        AtFlags::EMPTY_PATH,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )
    .map_err(|_| "cache_storage_filesystem_observation_failed")?;
    let mask = StatxFlags::from_bits_retain(observed.stx_mask);
    if !mask.contains(StatxFlags::BASIC_STATS | StatxFlags::MNT_ID)
        || observed.stx_mnt_id != expected.mount_id
        || observed.stx_dev_major != expected.statx_device_major
        || observed.stx_dev_minor != expected.statx_device_minor
        || rustix::fs::major(device) != expected.device_major
        || rustix::fs::minor(device) != expected.device_minor
    {
        return Err("cache_storage_filesystem_identity_mismatch");
    }
    let filesystem =
        fstatfs(directory).map_err(|_| "cache_storage_filesystem_observation_failed")?;
    if u64::try_from(filesystem.f_type).map_err(|_| "cache_storage_filesystem_identity_invalid")?
        != expected.fs_type_magic
    {
        return Err("cache_storage_filesystem_identity_mismatch");
    }
    let capacity =
        fstatvfs(directory).map_err(|_| "cache_storage_filesystem_observation_failed")?;
    if capacity.f_fsid != expected.filesystem_id || capacity.f_frsize == 0 {
        return Err("cache_storage_filesystem_identity_mismatch");
    }
    let available_bytes = capacity
        .f_bavail
        .checked_mul(capacity.f_frsize)
        .ok_or("cache_storage_filesystem_capacity_invalid")?;
    Ok(FreshCapacity {
        available_bytes,
        available_inodes: Some(capacity.f_favail),
        fragment_bytes: capacity.f_frsize,
    })
}

#[cfg(target_os = "linux")]
fn measure_tree(root: &rustix::fd::OwnedFd) -> Result<CacheTreeUsage, &'static str> {
    let root_stat = rustix::fs::fstat(root).map_err(|_| "cache_storage_tree_stat_failed")?;
    validate_directory(&root_stat)?;
    let mut state = MeasureState {
        root_device: root_stat.st_dev,
        entries: 0,
        seen_files: HashSet::new(),
        bytes: 0,
        inodes: 0,
    };
    state.add_object(&root_stat, false)?;
    walk(root, "", 0, &mut state)?;
    Ok(CacheTreeUsage {
        bytes: state.bytes,
        inodes: state.inodes,
    })
}

#[cfg(target_os = "linux")]
struct MeasureState {
    root_device: u64,
    entries: usize,
    seen_files: HashSet<(u64, u64)>,
    bytes: u64,
    inodes: u64,
}

#[cfg(target_os = "linux")]
impl MeasureState {
    fn add_object(&mut self, stat: &Stat, deduplicate: bool) -> Result<(), &'static str> {
        let identity = (stat.st_dev, stat.st_ino);
        if deduplicate && !self.seen_files.insert(identity) {
            return Ok(());
        }
        let allocated = stat
            .st_blocks
            .checked_mul(512)
            .and_then(|bytes| u64::try_from(bytes).ok())
            .ok_or("cache_storage_tree_limit")?;
        let size = u64::try_from(stat.st_size).map_err(|_| "cache_storage_tree_limit")?;
        self.bytes = self
            .bytes
            .checked_add(allocated.max(size))
            .ok_or("cache_storage_tree_limit")?;
        self.inodes = self
            .inodes
            .checked_add(1)
            .ok_or("cache_storage_tree_limit")?;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn walk(
    directory: &rustix::fd::OwnedFd,
    parent_path: &str,
    depth: usize,
    state: &mut MeasureState,
) -> Result<(), &'static str> {
    if depth > MAX_DEPTH {
        return Err("cache_storage_tree_limit");
    }
    let entries = Dir::read_from(directory).map_err(|_| "cache_storage_tree_read_failed")?;
    for item in entries {
        let item = item.map_err(|_| "cache_storage_tree_read_failed")?;
        let name = item.file_name();
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        state.entries = state
            .entries
            .checked_add(1)
            .filter(|count| *count <= MAX_ENTRIES)
            .ok_or("cache_storage_tree_limit")?;
        let name_text = name
            .to_str()
            .map_err(|_| "cache_storage_tree_path_invalid")?;
        let path = if parent_path.is_empty() {
            name_text.to_owned()
        } else {
            format!("{parent_path}/{name_text}")
        };
        if path.len() > PATH_LIMIT || path.bytes().any(|byte| byte.is_ascii_control()) {
            return Err("cache_storage_tree_path_invalid");
        }
        walk_entry(directory, name, &path, depth, state)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn walk_entry(
    directory: &rustix::fd::OwnedFd,
    name: &CStr,
    path: &str,
    depth: usize,
    state: &mut MeasureState,
) -> Result<(), &'static str> {
    let stat = statat(directory, name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|_| "cache_storage_tree_changed")?;
    if stat.st_dev != state.root_device || stat.st_mode & 0o7000_u32 != 0 {
        return Err("cache_storage_tree_entry_invalid");
    }
    match FileType::from_raw_mode(stat.st_mode) {
        FileType::Directory => {
            let (child, opened) = open_directory_entry(directory, name, &stat)?;
            state.add_object(&opened, false)?;
            walk(&child, path, depth + 1, state)?;
        }
        FileType::RegularFile => measure_regular_file(directory, name, &stat, state)?,
        FileType::Symlink => measure_symlink(directory, name, path, &stat, state)?,
        _ => return Err("cache_storage_tree_entry_invalid"),
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn open_directory_entry(
    directory: &rustix::fd::OwnedFd,
    name: &CStr,
    stat: &Stat,
) -> Result<(rustix::fd::OwnedFd, Stat), &'static str> {
    validate_directory(stat)?;
    let child = openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| "cache_storage_tree_changed")?;
    let opened = rustix::fs::fstat(&child).map_err(|_| "cache_storage_tree_stat_failed")?;
    if !same_object(stat, &opened) {
        return Err("cache_storage_tree_changed");
    }
    Ok((child, opened))
}

#[cfg(target_os = "linux")]
fn measure_regular_file(
    directory: &rustix::fd::OwnedFd,
    name: &CStr,
    stat: &Stat,
    state: &mut MeasureState,
) -> Result<(), &'static str> {
    if stat.st_uid != crate::process_owner::effective_uid() || stat.st_nlink == 0 {
        return Err("cache_storage_tree_entry_invalid");
    }
    let descriptor = openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| "cache_storage_tree_changed")?;
    let opened = rustix::fs::fstat(&descriptor).map_err(|_| "cache_storage_tree_stat_failed")?;
    if !crate::qualification_cache_payload::transfer::same_version(stat, &opened) {
        return Err("cache_storage_tree_changed");
    }
    state.add_object(&opened, true)
}

#[cfg(target_os = "linux")]
fn measure_symlink(
    directory: &rustix::fd::OwnedFd,
    name: &CStr,
    path: &str,
    stat: &Stat,
    state: &mut MeasureState,
) -> Result<(), &'static str> {
    if stat.st_uid != crate::process_owner::effective_uid() || stat.st_nlink != 1 {
        return Err("cache_storage_tree_entry_invalid");
    }
    let target = rustix::fs::readlinkat(directory, name, Vec::new())
        .map_err(|_| "cache_storage_tree_read_failed")?
        .into_string()
        .map_err(|_| "cache_storage_tree_path_invalid")?;
    payload::validate_internal_link(path, &target)
        .map_err(|_| "cache_storage_tree_entry_invalid")?;
    state.add_object(stat, false)
}

#[cfg(target_os = "linux")]
fn validate_directory(stat: &Stat) -> Result<(), &'static str> {
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        || stat.st_uid != crate::process_owner::effective_uid()
        || stat.st_mode & 0o7000_u32 != 0
    {
        return Err("cache_storage_tree_entry_invalid");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn same_object(left: &Stat, right: &Stat) -> bool {
    left.st_dev == right.st_dev && left.st_ino == right.st_ino
}
