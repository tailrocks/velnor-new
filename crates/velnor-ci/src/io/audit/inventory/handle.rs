// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use nix::dir::Dir;
use nix::fcntl::{AtFlags, OFlag, openat, readlinkat};
use nix::sys::stat::{FileStat, Mode, SFlag, fstat, fstatat};

use super::Kind;
use super::resolve;
use super::walk::unresolved_link;

pub(super) type Opened = Rc<Dir>;
pub(super) type Identity = (libc::dev_t, libc::ino_t);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::io::audit) struct Stamp {
    device: i128,
    inode: i128,
    modified: (i128, i128),
    changed: (i128, i128),
}

impl Stamp {
    pub(super) fn is(&self, (device, inode): Identity) -> bool {
        self.device == i128::from(device) && self.inode == i128::from(inode)
    }
}

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

pub(super) struct Observed {
    pub(super) kind: Kind,
    pub(super) changed: Option<SystemTime>,
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
pub(super) fn stamp(directory: &Dir) -> io::Result<Stamp> {
    let stat = fstat(directory).map_err(failure)?;
    Ok(Stamp {
        device: i128::from(stat.st_dev),
        inode: i128::from(stat.st_ino),
        modified: (i128::from(stat.st_mtime), i128::from(stat.st_mtime_nsec)),
        changed: (i128::from(stat.st_ctime), i128::from(stat.st_ctime_nsec)),
    })
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
    Ok(classify(directory, name, path, false, &|| {})?.map(|observed| observed.kind))
}

pub(super) fn observe(
    directory: impl AsFd,
    name: &OsStr,
    path: &Path,
    resolved: &dyn Fn(),
) -> io::Result<Option<Observed>> {
    classify(directory, name, path, true, resolved)
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

fn classify(
    directory: impl AsFd,
    name: &OsStr,
    path: &Path,
    with_changed: bool,
    after_resolve: &dyn Fn(),
) -> io::Result<Option<Observed>> {
    let stat = match fstatat(&directory, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
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
    let target = match readlinkat(&directory, name) {
        Ok(target) => PathBuf::from(target),
        Err(nix::errno::Errno::ENOENT) => return Ok(None),
        Err(errno) => return Err(failure(errno)),
    };
    let base = path.parent().unwrap_or(path);
    let followed = match resolve::resolve(directory.as_fd(), base, &target) {
        Ok(followed) => Some(followed),
        Err(error) if unresolved_link(&error) => None,
        Err(error) => return Err(error),
    };
    after_resolve();
    let changed = if with_changed {
        let observed = followed.as_ref().map_or(&stat, |followed| &followed.stat);
        (format_of(observed) != SFlag::S_IFDIR).then(|| changed(observed))
    } else {
        None
    };
    let resolved = followed.map(|followed| followed.path);
    Ok(Some(Observed {
        kind: Kind::Link { target, resolved },
        changed,
    }))
}
