// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::unix::ffi::OsStringExt;
use std::path::Path;
use std::rc::Rc;

use nix::dir::Dir;
use nix::fcntl::{AtFlags, OFlag, openat, readlinkat};
use nix::sys::stat::{Mode, SFlag, fstatat};

use super::Kind;
use super::walk::unresolved_link;

pub(super) type Opened = Rc<Dir>;

pub(super) enum Origin {
    Path,
    Child { parent: Opened, name: OsString },
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
        Origin::Path => openat(nix::fcntl::AT_FDCWD, path, flags, Mode::empty()),
        Origin::Child { parent, name } => {
            openat(parent.as_ref(), name.as_os_str(), flags, Mode::empty())
        }
    }
    .map_err(failure)?;
    Dir::from_fd(fd).map_err(failure)
}

pub(super) fn names(directory: &mut Dir) -> io::Result<Vec<OsString>> {
    let mut names = Vec::new();
    for entry in directory.iter() {
        let name = entry.map_err(failure)?.file_name().to_bytes().to_vec();
        if name != b"." && name != b".." {
            names.push(OsString::from_vec(name));
        }
    }
    Ok(names)
}

pub(super) fn kind_at(directory: &Dir, name: &OsStr, path: &Path) -> io::Result<Option<Kind>> {
    let stat = match fstatat(directory, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
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
    match readlinkat(directory, name) {
        Ok(target) => Ok(Some(Kind::Link {
            target: target.into(),
            resolved,
        })),
        Err(nix::errno::Errno::ENOENT) => Ok(None),
        Err(errno) => Err(failure(errno)),
    }
}
