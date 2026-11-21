// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::{Component, Path, PathBuf};

use nix::errno::Errno;
use nix::fcntl::{AT_FDCWD, AtFlags, OFlag, openat, readlinkat};
use nix::sys::stat::{Mode, SFlag, fstatat};

const MAX_LINKS: usize = 40;

fn failure(errno: Errno) -> io::Error {
    io::Error::from_raw_os_error(errno as i32)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
const TRAVERSE: OFlag = OFlag::O_PATH;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
const TRAVERSE: OFlag = OFlag::O_SEARCH;

fn open_directory(anchor: BorrowedFd<'_>, name: &Path, listing: bool) -> io::Result<OwnedFd> {
    let access = if listing { OFlag::O_RDONLY } else { TRAVERSE };
    let flags = access | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
    openat(anchor, name, flags, Mode::empty()).map_err(failure)
}

pub(super) fn open_path(path: &Path, listing: bool) -> io::Result<OwnedFd> {
    let mut components = path.components();
    if components.next() != Some(Component::RootDir) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not an absolute path", path.display()),
        ));
    }
    let mut remaining = components.as_path().components().count();
    let mut current = open_directory(AT_FDCWD, Path::new("/"), listing && remaining == 0)?;
    for component in components {
        remaining -= 1;
        let Component::Normal(name) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} is not a canonical path", path.display()),
            ));
        };
        current = open_directory(current.as_fd(), Path::new(name), listing && remaining == 0)?;
    }
    Ok(current)
}

enum Step {
    Root,
    Parent,
    Name(OsString),
}

fn steps(target: &Path, pending: &mut Vec<Step>) {
    for component in target.components().rev() {
        match component {
            Component::Normal(name) => pending.push(Step::Name(name.to_owned())),
            Component::ParentDir => pending.push(Step::Parent),
            Component::RootDir => pending.push(Step::Root),
            Component::CurDir | Component::Prefix(_) => {}
        }
    }
}

pub(super) fn resolve(
    directory: BorrowedFd<'_>,
    base: &Path,
    target: &Path,
) -> io::Result<PathBuf> {
    let mut current = directory.try_clone_to_owned()?;
    let mut trail: Vec<OwnedFd> = Vec::new();
    let mut path = base.to_path_buf();
    let mut pending = Vec::new();
    let mut links = 0;
    steps(target, &mut pending);
    while let Some(step) = pending.pop() {
        match step {
            Step::Root => {
                current = open_path(Path::new("/"), false)?;
                trail.clear();
                path = PathBuf::from("/");
            }
            Step::Parent => {
                current = match trail.pop() {
                    Some(parent) => parent,
                    None => open_directory(current.as_fd(), Path::new(".."), false)?,
                };
                path.pop();
            }
            Step::Name(name) => {
                let stat = fstatat(&current, name.as_os_str(), AtFlags::AT_SYMLINK_NOFOLLOW)
                    .map_err(failure)?;
                let format = SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT;
                if format == SFlag::S_IFLNK {
                    links += 1;
                    if links > MAX_LINKS {
                        return Err(failure(Errno::ELOOP));
                    }
                    let next =
                        PathBuf::from(readlinkat(&current, name.as_os_str()).map_err(failure)?);
                    if next.as_os_str().is_empty() {
                        return Err(failure(Errno::ENOENT));
                    }
                    steps(&next, &mut pending);
                } else if format == SFlag::S_IFDIR {
                    let opened = open_directory(current.as_fd(), Path::new(&name), false)?;
                    trail.push(std::mem::replace(&mut current, opened));
                    path.push(&name);
                } else if pending.is_empty() {
                    path.push(&name);
                } else {
                    return Err(failure(Errno::ENOTDIR));
                }
            }
        }
    }
    Ok(path)
}
