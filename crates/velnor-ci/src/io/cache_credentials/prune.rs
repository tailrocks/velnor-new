// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{CStr, CString, OsStr};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use nix::dir::Dir;
use nix::fcntl::{AtFlags, OFlag};
use nix::sys::stat::{Mode, SFlag, fstatat};
use nix::unistd::{UnlinkatFlags, unlinkat};

use super::{MAX_DEPTH, leave_out, shown};
use crate::escape::inert;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Identity {
    device: u64,
    inode: u64,
}

impl Identity {
    pub(super) fn of(metadata: &fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }
}

fn no_follow() -> OFlag {
    OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC
}

fn refuse(path: &Path, what: &str, reason: &str) -> String {
    format!("cannot {what} {}: {}", shown(path), inert(reason))
}

pub(super) fn remove_tree(
    path: &Path,
    expected: Identity,
    depth: usize,
    budget: &mut usize,
) -> Option<String> {
    let file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(no_follow().bits())
        .open(path)
    {
        Ok(file) => file,
        Err(error) => return Some(refuse(path, "open", &error.to_string())),
    };
    match file.metadata() {
        Ok(metadata) if Identity::of(&metadata) == expected => {}
        Ok(_) => {
            return Some(format!(
                "{} was replaced while it was being left out",
                shown(path)
            ));
        }
        Err(error) => return Some(refuse(path, "inspect", &error.to_string())),
    }
    let mut directory = match Dir::from_fd(file.into()) {
        Ok(directory) => directory,
        Err(error) => return Some(refuse(path, "open", &error.to_string())),
    };
    if let Some(found) = empty(&mut directory, path, depth, budget) {
        return Some(found);
    }
    leave_out(path, fs::remove_dir(path))
}

fn names(directory: &mut Dir, path: &Path, budget: &mut usize) -> Result<Vec<CString>, String> {
    let mut names = Vec::new();
    for entry in directory.iter() {
        let entry = entry.map_err(|error| refuse(path, "list", &error.to_string()))?;
        let name = entry.file_name();
        if name == c"." || name == c".." {
            continue;
        }
        let Some(left) = budget.checked_sub(1) else {
            return Err(format!("{} holds too many entries to scan", shown(path)));
        };
        *budget = left;
        names.push(name.to_owned());
    }
    Ok(names)
}

fn is_directory(mode: nix::libc::mode_t) -> bool {
    SFlag::from_bits_truncate(mode) & SFlag::S_IFMT == SFlag::S_IFDIR
}

fn empty(directory: &mut Dir, path: &Path, depth: usize, budget: &mut usize) -> Option<String> {
    let names = match names(directory, path, budget) {
        Ok(names) => names,
        Err(reason) => return Some(reason),
    };
    for name in names {
        let child = path.join(OsStr::from_bytes(name.to_bytes()));
        let found = match fstatat(&*directory, name.as_c_str(), AtFlags::AT_SYMLINK_NOFOLLOW) {
            Ok(stat) if is_directory(stat.st_mode) => {
                descend(directory, &name, &child, depth, budget)
            }
            Ok(_) => unlink(directory, &name, &child, UnlinkatFlags::NoRemoveDir),
            Err(error) => Some(refuse(&child, "inspect", &error.to_string())),
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

pub(super) fn descend(
    parent: &Dir,
    name: &CStr,
    path: &Path,
    depth: usize,
    budget: &mut usize,
) -> Option<String> {
    if depth + 1 > MAX_DEPTH {
        return Some(format!("{} is nested too deeply to scan", shown(path)));
    }
    let mut inner = match Dir::openat(parent, name, no_follow(), Mode::empty()) {
        Ok(inner) => inner,
        Err(error) => return Some(refuse(path, "open", &error.to_string())),
    };
    if let Some(found) = empty(&mut inner, path, depth + 1, budget) {
        return Some(found);
    }
    drop(inner);
    unlink(parent, name, path, UnlinkatFlags::RemoveDir)
}

fn unlink(parent: &Dir, name: &CStr, path: &Path, flag: UnlinkatFlags) -> Option<String> {
    leave_out(
        path,
        unlinkat(parent, name, flag).map_err(std::io::Error::from),
    )
}
