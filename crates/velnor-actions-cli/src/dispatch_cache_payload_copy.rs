//! Copy a validated payload tree without following its entries.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path};

use rustix::fs::{AtFlags, FileType, Mode, OFlags, Stat, fchmod, open, openat, statat};

use super::super::{CreatedPayload, cleanup};
use super::digest_open_directory;

#[path = "dispatch_cache_payload_copy_walk.rs"]
mod walk;

/// Copy a non-empty validated tree into a new directory.
///
/// The destination parent must exist and belong to the current user. The
/// function preserves internal symbolic links, hard links, and file modes.
pub(crate) fn copy_tree_to_new_dir(
    source: &Path,
    destination: &Path,
) -> Result<String, &'static str> {
    copy_tree_with_hook_owned(source, destination, |_| {}).map(|created| created.digest)
}

#[cfg(test)]
fn copy_tree_with_hook<F>(
    source: &Path,
    destination: &Path,
    mut after_source_open: F,
) -> Result<String, &'static str>
where
    F: FnMut(&Path),
{
    copy_tree_with_hook_owned(source, destination, &mut after_source_open)
        .map(|created| created.digest)
}

pub(crate) fn copy_tree_to_new_dir_owned(
    source: &Path,
    destination: &Path,
) -> Result<CreatedPayload, &'static str> {
    copy_tree_with_hook_owned(source, destination, |_| {})
}

fn copy_tree_with_hook_owned<F>(
    source: &Path,
    destination: &Path,
    mut after_source_open: F,
) -> Result<CreatedPayload, &'static str>
where
    F: FnMut(&Path),
{
    let source_fd = open_directory(source)?;
    let source_stat = fstat(&source_fd)?;
    let expected = digest_directory_fd(&source_fd)?.ok_or("qualification_cache_payload_empty")?;
    let destination_parent = destination
        .parent()
        .ok_or("qualification_cache_payload_destination_invalid")?;
    let destination_name = destination
        .file_name()
        .ok_or("qualification_cache_payload_destination_invalid")?;
    validate_destination(source, destination, destination_name)?;
    let parent_fd = open_directory(destination_parent)?;
    let name = CString::new(destination_name.as_bytes())
        .map_err(|_| "qualification_cache_payload_destination_invalid")?;
    rustix::fs::mkdirat(
        &parent_fd,
        name.as_c_str(),
        Mode::RUSR | Mode::WUSR | Mode::XUSR,
    )
    .map_err(|_| "qualification_cache_payload_destination_exists")?;
    let created = match statat(&parent_fd, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) if FileType::from_raw_mode(stat.st_mode) == FileType::Directory => stat,
        _ => return Err("qualification_cache_payload_open_failed"),
    };
    let destination_fd = open_directory_at(&parent_fd, name.as_c_str()).ok();
    let destination_fd =
        destination_fd.filter(|fd| fstat(fd).is_ok_and(|opened| same_object(&created, &opened)));
    let Some(destination_fd) = destination_fd else {
        return Err("qualification_cache_payload_open_failed");
    };
    let mut owned = CreatedPayload {
        digest: String::new(),
        descriptor: destination_fd,
        stat: created,
    };
    let copied = copy_created_tree(
        &source_fd,
        &owned.descriptor,
        &source_stat,
        &expected,
        &mut after_source_open,
    );
    let result = if copied.is_ok()
        && (!same_directory_at_path(source, &source_fd)
            || !same_directory_at_path(destination_parent, &parent_fd)
            || !same_entry_at_path(&parent_fd, name.as_c_str(), &owned.stat))
    {
        Err("qualification_cache_payload_file_changed")
    } else {
        copied
    };
    match result {
        Ok(digest) => {
            owned.digest = digest;
            Ok(owned)
        }
        Err(error) => {
            cleanup::remove_owned_at(&parent_fd, name.as_c_str(), &owned)?;
            Err(error)
        }
    }
}

fn validate_destination(
    source: &Path,
    destination: &Path,
    destination_name: &std::ffi::OsStr,
) -> Result<(), &'static str> {
    if !source.is_absolute()
        || !destination.is_absolute()
        || destination_name.is_empty()
        || destination_name
            .as_bytes()
            .iter()
            .any(|byte| matches!(*byte, b'/' | b'\0'))
        || source == destination
        || source.starts_with(destination)
        || destination.starts_with(source)
    {
        return Err("qualification_cache_payload_destination_invalid");
    }
    Ok(())
}

fn copy_created_tree<F>(
    source_fd: &rustix::fd::OwnedFd,
    destination_fd: &rustix::fd::OwnedFd,
    source_stat: &Stat,
    expected_digest: &str,
    after_source_open: &mut F,
) -> Result<String, &'static str>
where
    F: FnMut(&Path),
{
    let mut context = walk::CopyContext::new(destination_fd, after_source_open);
    walk::copy_directory(source_fd, destination_fd, 0, "", &mut context)?;
    fchmod(
        destination_fd,
        Mode::from_raw_mode(source_stat.st_mode & 0o777),
    )
    .map_err(|_| "qualification_cache_payload_write_failed")?;
    if digest_directory_fd(destination_fd)?.as_deref() != Some(expected_digest) {
        return Err("qualification_cache_payload_copy_mismatch");
    }
    let source_end = fstat(source_fd)?;
    if !super::same_version(source_stat, &source_end)
        || digest_directory_fd(source_fd)?.as_deref() != Some(expected_digest)
    {
        return Err("qualification_cache_payload_file_changed");
    }
    Ok(expected_digest.to_owned())
}

fn digest_directory_fd(directory: &rustix::fd::OwnedFd) -> Result<Option<String>, &'static str> {
    let independent = open_directory_at(directory, c".")?;
    digest_open_directory(&independent)
}

fn same_directory_at_path(path: &Path, expected: &rustix::fd::OwnedFd) -> bool {
    let Ok(current) = open_directory(path) else {
        return false;
    };
    fstat(expected).is_ok_and(|left| fstat(&current).is_ok_and(|right| same_object(&left, &right)))
}

fn same_entry_at_path(
    parent: &rustix::fd::OwnedFd,
    name: &std::ffi::CStr,
    expected: &Stat,
) -> bool {
    statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)
        .is_ok_and(|current| same_object(expected, &current))
}

fn open_directory_at(
    parent: &rustix::fd::OwnedFd,
    name: &std::ffi::CStr,
) -> Result<rustix::fd::OwnedFd, &'static str> {
    openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "qualification_cache_payload_open_failed")
}

fn open_directory(path: &Path) -> Result<rustix::fd::OwnedFd, &'static str> {
    if !path.is_absolute() {
        return Err("qualification_cache_payload_path_invalid");
    }
    let mut fd = open(
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "qualification_cache_payload_open_failed")?;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                let name = CString::new(name.as_bytes())
                    .map_err(|_| "qualification_cache_payload_path_invalid")?;
                fd = open_directory_at(&fd, name.as_c_str())
                    .map_err(|_| "qualification_cache_payload_path_invalid")?;
            }
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                return Err("qualification_cache_payload_path_invalid");
            }
        }
    }
    let metadata = fstat(&fd)?;
    super::validate_directory(&metadata)?;
    Ok(fd)
}

fn fstat(fd: &rustix::fd::OwnedFd) -> Result<Stat, &'static str> {
    rustix::fs::fstat(fd).map_err(|_| "qualification_cache_payload_open_failed")
}

fn same_object(left: &Stat, right: &Stat) -> bool {
    left.st_dev == right.st_dev && left.st_ino == right.st_ino
}

#[cfg(test)]
#[path = "../tests/dispatch_cache_payload_copy_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "../tests/dispatch_cache_payload_tests.rs"]
mod digest_tests;
