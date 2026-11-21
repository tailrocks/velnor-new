// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use nix::dir::Dir;
use nix::fcntl::{AtFlags, readlinkat};
use nix::fcntl::{OFlag, openat};
use nix::sys::stat::Mode;
use nix::sys::stat::{SFlag, fstat, fstatat};

use super::Kind;
use super::resolve;
use super::walk::unresolved_link;

pub(super) type Opened = Rc<Dir>;
pub(super) type Identity = (libc::dev_t, libc::ino_t);

#[derive(Clone)]
pub(super) enum Origin {
    Path,
    Child { parent: Opened, name: OsString },
}
pub(super) enum Current {
    Gone,
    Directory(Identity),
    Other(Kind),
}

fn failure(errno: nix::errno::Errno) -> io::Error {
    io::Error::from_raw_os_error(errno as i32)
}

pub(super) fn replaced(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    ) || error.raw_os_error() == Some(libc::ELOOP)
}

pub(super) fn open(origin: &Origin, path: &Path) -> io::Result<Dir> {
    let fd = match origin {
        Origin::Path => resolve::open_path(path, true)?,
        Origin::Child { parent, name } => {
            let flags = OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
            openat(parent.as_ref(), name.as_os_str(), flags, Mode::empty()).map_err(failure)?
        }
    };
    Dir::from_fd(fd).map_err(failure)
}
pub(super) fn identity(directory: &Dir) -> io::Result<Identity> {
    let stat = fstat(directory).map_err(failure)?;
    Ok((stat.st_dev, stat.st_ino))
}
pub(super) fn current(origin: &Origin, path: &Path) -> io::Result<Current> {
    let held: OwnedFd;
    let (anchor, name) = match origin {
        Origin::Child { parent, name } => (parent.as_fd(), name.as_os_str()),
        Origin::Path => {
            let (container, name) = match (path.parent(), path.file_name()) {
                (Some(container), Some(name)) => (container, name),
                _ => (path, OsStr::new(".")),
            };
            held = match resolve::open_path(container, false) {
                Ok(held) => held,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Ok(Current::Gone);
                }
                Err(error) => return Err(error),
            };
            (held.as_fd(), name)
        }
    };
    let stat = match fstatat(anchor, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(nix::errno::Errno::ENOENT) => return Ok(Current::Gone),
        Err(errno) => return Err(failure(errno)),
    };
    let format = SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT;
    if format == SFlag::S_IFDIR {
        return Ok(Current::Directory((stat.st_dev, stat.st_ino)));
    }
    Ok(match kind_at(anchor, name, path)? {
        Some(kind) => Current::Other(kind),
        None => Current::Gone,
    })
}

pub(super) fn names(
    directory: &mut Dir,
    mut accept: impl FnMut(&OsStr) -> io::Result<()>,
) -> io::Result<Vec<OsString>> {
    let mut names = Vec::new();
    for entry in directory.iter() {
        let name = entry.map_err(failure)?.file_name().to_bytes().to_vec();
        if name != b"." && name != b".." {
            let name = OsString::from_vec(name);
            accept(&name)?;
            names.push(name);
        }
    }
    Ok(names)
}

pub(super) fn kind_at(directory: impl AsFd, name: &OsStr, path: &Path) -> io::Result<Option<Kind>> {
    let stat = match fstatat(&directory, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(nix::errno::Errno::ENOENT) => return Ok(None),
        Err(errno) => return Err(failure(errno)),
    };
    let format = SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT;
    if format != SFlag::S_IFLNK {
        return Ok(Some(if format == SFlag::S_IFDIR {
            Kind::Directory
        } else if format == SFlag::S_IFREG {
            Kind::File
        } else {
            Kind::Other
        }));
    }
    let target = match readlinkat(&directory, name) {
        Ok(target) => PathBuf::from(target),
        Err(nix::errno::Errno::ENOENT) => return Ok(None),
        Err(errno) => return Err(failure(errno)),
    };
    let base = path.parent().unwrap_or(path);
    let resolved = match resolve::resolve(directory.as_fd(), base, &target) {
        Ok(resolved) => Some(resolved),
        Err(error) if unresolved_link(&error) => None,
        Err(error) => return Err(error),
    };
    Ok(Some(Kind::Link { target, resolved }))
}
