// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::unix::ffi::OsStringExt;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use nix::dir::Dir;
use nix::fcntl::{AtFlags, OFlag, openat, readlinkat};
use nix::sys::stat::{FileStat, Mode, SFlag, fstat, fstatat};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::io::audit) struct Identity {
    device: i128,
    inode: i128,
    modified: (i128, i128),
    changed: (i128, i128),
}

pub(super) fn identity(directory: &Dir) -> io::Result<Identity> {
    let stat = fstat(directory).map_err(failure)?;
    Ok(Identity {
        device: i128::from(stat.st_dev),
        inode: i128::from(stat.st_ino),
        modified: (i128::from(stat.st_mtime), i128::from(stat.st_mtime_nsec)),
        changed: (i128::from(stat.st_ctime), i128::from(stat.st_ctime_nsec)),
    })
}

pub(super) struct Observed {
    pub(super) kind: Kind,
    pub(super) changed: Option<SystemTime>,
}

fn instant(seconds: i64, nanos: i64) -> SystemTime {
    let nanos = Duration::from_nanos(u64::try_from(nanos).unwrap_or(0));
    match u64::try_from(seconds) {
        Ok(seconds) => SystemTime::UNIX_EPOCH + Duration::from_secs(seconds) + nanos,
        Err(_) => SystemTime::UNIX_EPOCH - Duration::from_secs(seconds.unsigned_abs()) + nanos,
    }
}

fn changed(stat: &FileStat) -> SystemTime {
    let modified = instant(stat.st_mtime, stat.st_mtime_nsec);
    let inode = instant(stat.st_ctime, stat.st_ctime_nsec);
    modified.max(inode)
}

fn format_of(stat: &FileStat) -> SFlag {
    SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT
}

pub(super) fn kind_at(directory: &Dir, name: &OsStr, path: &Path) -> io::Result<Option<Kind>> {
    Ok(classify(directory, name, path, false)?.map(|observed| observed.kind))
}

pub(super) fn observe(directory: &Dir, name: &OsStr, path: &Path) -> io::Result<Option<Observed>> {
    classify(directory, name, path, true)
}

fn classify(
    directory: &Dir,
    name: &OsStr,
    path: &Path,
    with_modified: bool,
) -> io::Result<Option<Observed>> {
    let stat = match fstatat(directory, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(nix::errno::Errno::ENOENT) => return Ok(None),
        Err(errno) => return Err(failure(errno)),
    };
    let format = format_of(&stat);
    if format != SFlag::S_IFLNK {
        let kind = if format == SFlag::S_IFDIR {
            Kind::Directory
        } else if format == SFlag::S_IFREG {
            Kind::File
        } else {
            Kind::Other
        };
        let changed = (format != SFlag::S_IFDIR).then(|| changed(&stat));
        return Ok(Some(Observed { kind, changed }));
    }
    let resolved = match std::fs::canonicalize(path) {
        Ok(resolved) => Some(resolved),
        Err(error) if unresolved_link(&error) => None,
        Err(error) => return Err(error),
    };
    let changed = if with_modified {
        let followed = match fstatat(directory, name, AtFlags::empty()) {
            Ok(followed) => followed,
            Err(
                nix::errno::Errno::ENOENT | nix::errno::Errno::ELOOP | nix::errno::Errno::ENOTDIR,
            ) => stat,
            Err(errno) => return Err(failure(errno)),
        };
        (format_of(&followed) != SFlag::S_IFDIR).then(|| changed(&followed))
    } else {
        None
    };
    match readlinkat(directory, name) {
        Ok(target) => Ok(Some(Observed {
            kind: Kind::Link {
                target: target.into(),
                resolved,
            },
            changed,
        })),
        Err(nix::errno::Errno::ENOENT) => Ok(None),
        Err(errno) => Err(failure(errno)),
    }
}
