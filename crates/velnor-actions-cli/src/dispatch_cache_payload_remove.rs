//! Remove a partial tree through its open directory descriptors.

use std::ffi::CStr;

use rustix::fs::{AtFlags, Dir, FileType, Mode, OFlags, Stat, openat, statat, unlinkat};

use super::super::{MAX_DEPTH, MAX_ENTRIES};
use super::same_object;

pub(super) fn remove_created_tree(parent: &rustix::fd::OwnedFd, name: &CStr, expected: &Stat) {
    let Ok(named) = statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) else {
        return;
    };
    if !same_object(expected, &named)
        || FileType::from_raw_mode(named.st_mode) != FileType::Directory
    {
        return;
    }
    let Ok(root) = openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    ) else {
        return;
    };
    if rustix::fs::fstat(&root).is_ok_and(|current| same_object(expected, &current)) {
        remove_contents(&root, 0);
    }
    if statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)
        .is_ok_and(|current| same_object(expected, &current))
    {
        let _ = unlinkat(parent, name, AtFlags::REMOVEDIR);
    }
}

fn remove_contents(directory_fd: &rustix::fd::OwnedFd, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(directory) = Dir::read_from(directory_fd) else {
        return;
    };
    let mut names = Vec::new();
    for entry in directory {
        if names.len() >= MAX_ENTRIES {
            return;
        }
        let Ok(entry) = entry else {
            return;
        };
        let name = entry.file_name().to_owned();
        if name.to_bytes() != b"." && name.to_bytes() != b".." {
            names.push(name);
        }
    }
    for name in names {
        let Ok(before) = statat(directory_fd, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW) else {
            continue;
        };
        if FileType::from_raw_mode(before.st_mode) == FileType::Directory {
            remove_child_directory(directory_fd, name.as_c_str(), &before, depth + 1);
        } else if statat(directory_fd, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)
            .is_ok_and(|current| same_object(&before, &current))
        {
            let _ = unlinkat(directory_fd, name.as_c_str(), AtFlags::empty());
        }
    }
}

fn remove_child_directory(
    parent: &rustix::fd::OwnedFd,
    name: &CStr,
    expected: &Stat,
    depth: usize,
) {
    let Ok(child) = openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    ) else {
        return;
    };
    if !rustix::fs::fstat(&child).is_ok_and(|current| same_object(expected, &current)) {
        return;
    }
    remove_contents(&child, depth);
    if statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)
        .is_ok_and(|current| same_object(expected, &current))
    {
        let _ = unlinkat(parent, name, AtFlags::REMOVEDIR);
    }
}
