// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::ffi::OsStringExt;
use std::path::Path;
use std::rc::Rc;

use nix::dir::Dir;
use nix::fcntl::{AT_FDCWD, AtFlags, OFlag, openat, readlinkat};
use nix::sys::stat::{Mode, SFlag, fstat, fstatat};

use super::Kind;
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
    let flags = OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
    let fd = match origin {
        Origin::Path => openat(AT_FDCWD, path, flags, Mode::empty()),
        Origin::Child { parent, name } => {
            openat(parent.as_ref(), name.as_os_str(), flags, Mode::empty())
        }
    }
    .map_err(failure)?;
    Dir::from_fd(fd).map_err(failure)
}
pub(super) fn identity(directory: &Dir) -> io::Result<Identity> {
    let stat = fstat(directory).map_err(failure)?;
    Ok((stat.st_dev, stat.st_ino))
}
pub(super) fn current(origin: &Origin, path: &Path) -> io::Result<Current> {
    let (anchor, name) = anchor(origin, path);
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

fn anchor<'a>(origin: &'a Origin, path: &'a Path) -> (BorrowedFd<'a>, &'a OsStr) {
    match origin {
        Origin::Path => (AT_FDCWD, path.as_os_str()),
        Origin::Child { parent, name } => (parent.as_fd(), name.as_os_str()),
    }
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
    let resolved = match std::fs::canonicalize(path) {
        Ok(resolved) => Some(resolved),
        Err(error) if unresolved_link(&error) => None,
        Err(error) => return Err(error),
    };
    match readlinkat(&directory, name) {
        Ok(target) => Ok(Some(Kind::Link {
            target: target.into(),
            resolved,
        })),
        Err(nix::errno::Errno::ENOENT) => Ok(None),
        Err(errno) => Err(failure(errno)),
    }
}
