// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use nix::dir::Dir;
use nix::fcntl::{AtFlags, OFlag, openat, readlinkat};
use nix::sys::stat::{FileStat, Mode, SFlag, fstat, fstatat};

use super::Kind;
use super::resolve::{self, Reach};
use super::walk::{changed, unresolved_link};

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
    fn of(stat: &FileStat) -> Self {
        Self {
            device: i128::from(stat.st_dev),
            inode: i128::from(stat.st_ino),
            modified: (i128::from(stat.st_mtime), i128::from(stat.st_mtime_nsec)),
            changed: (i128::from(stat.st_ctime), i128::from(stat.st_ctime_nsec)),
        }
    }

    pub(super) fn is(&self, (device, inode): Identity) -> bool {
        self.device == i128::from(device) && self.inode == i128::from(inode)
    }
}

#[derive(Clone)]
pub(super) enum Origin {
    Path,
    Child {
        parent: Opened,
        stamp: Stamp,
        name: OsString,
    },
    Reopen {
        parent: PathBuf,
        stamp: Stamp,
        child: Option<Stamp>,
        name: OsString,
    },
}
pub(super) struct Anchor {
    pub(super) dir: Opened,
    pub(super) path: PathBuf,
}

pub(super) struct Climb<'a> {
    pub(super) anchor: Option<Anchor>,
    #[cfg(test)]
    pub(super) stepped: &'a dyn Fn(&Path, usize),
    #[cfg(not(test))]
    pub(super) marker: std::marker::PhantomData<&'a ()>,
}

impl Climb<'_> {
    pub(super) fn none() -> Self {
        Self {
            anchor: None,
            #[cfg(test)]
            stepped: &|_, _| {},
            #[cfg(not(test))]
            marker: std::marker::PhantomData,
        }
    }
}

pub(super) enum Current {
    Gone,
    Directory(Identity),
    Other(Kind),
}

pub(super) struct Observed {
    pub(super) kind: Kind,
    pub(super) modified: Option<SystemTime>,
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

pub(super) fn open(origin: &Origin, path: &Path, climb: &Climb<'_>) -> io::Result<Dir> {
    let fd = match origin {
        Origin::Path => resolve::open_path(path, true)?,
        Origin::Child { parent, name, .. } => child(parent.as_fd(), name)?,
        Origin::Reopen {
            parent,
            stamp,
            child: recorded,
            name,
        } => {
            let opened = child(reopen(parent, stamp, climb)?.as_fd(), name)?;
            if self::stamp(&opened).ok() != *recorded {
                return Err(changed(io::ErrorKind::Other, parent));
            }
            opened
        }
    };
    Dir::from_fd(fd).map_err(failure)
}

pub(super) fn child_stamp(parent: &Opened, name: &OsStr) -> Option<Stamp> {
    fstatat(parent.as_fd(), name, AtFlags::AT_SYMLINK_NOFOLLOW)
        .ok()
        .map(|stat| Stamp::of(&stat))
}

fn child(parent: BorrowedFd<'_>, name: &OsStr) -> io::Result<OwnedFd> {
    let flags = OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
    openat(parent, name, flags, Mode::empty()).map_err(failure)
}

fn reopen(parent: &Path, expected: &Stamp, climb: &Climb<'_>) -> io::Result<OwnedFd> {
    let below = climb
        .anchor
        .as_ref()
        .and_then(|anchor| Some((anchor, parent.strip_prefix(&anchor.path).ok()?)));
    #[cfg(test)]
    {
        let steps = below.map_or_else(
            || parent.components().count(),
            |(_, below)| below.components().count(),
        );
        (climb.stepped)(parent, steps);
    }
    let opened = match below {
        Some((anchor, below)) => resolve::open_below(anchor.dir.as_fd(), below, false),
        None => resolve::open_path(parent, false),
    };
    let held = opened.map_err(|error| {
        if replaced(&error) {
            changed(io::ErrorKind::Other, parent)
        } else {
            error
        }
    })?;
    if expected.is(identity(&held)?) {
        Ok(held)
    } else {
        Err(changed(io::ErrorKind::Other, parent))
    }
}

pub(super) fn revisited(
    identities: &BTreeMap<PathBuf, Stamp>,
    directory: &Path,
    reach: Reach<'_>,
) -> io::Result<()> {
    let Some(recorded) = identities.get(directory) else {
        return Ok(());
    };
    match current(&Origin::Path, directory, reach, &Climb::none()) {
        Ok(Current::Directory(now)) if recorded.is(now) => Ok(()),
        Ok(_) => Err(changed(io::ErrorKind::Other, directory)),
        Err(error) if replaced(&error) => Err(changed(error.kind(), directory)),
        Err(error) => Err(error),
    }
}

fn identity(directory: impl AsFd) -> io::Result<Identity> {
    let stat = fstat(directory).map_err(failure)?;
    Ok((stat.st_dev, stat.st_ino))
}

pub(super) fn is_parent_of(path: &Path, parent: BorrowedFd<'_>) -> io::Result<()> {
    let moved = || changed(io::ErrorKind::Other, path);
    let lexical = resolve::open_path(path.parent().unwrap_or(path), false).map_err(|_| moved())?;
    if stamp(&lexical)? == stamp(parent)? {
        Ok(())
    } else {
        Err(moved())
    }
}

pub(super) fn stamp(directory: impl AsFd) -> io::Result<Stamp> {
    fstat(directory)
        .map(|stat| Stamp::of(&stat))
        .map_err(failure)
}
pub(super) fn current(
    origin: &Origin,
    path: &Path,
    reach: Reach<'_>,
    climb: &Climb<'_>,
) -> io::Result<Current> {
    let held: OwnedFd;
    let (anchor, name) = match origin {
        Origin::Child { parent, name, .. } => (parent.as_fd(), name.as_os_str()),
        Origin::Reopen {
            parent,
            stamp,
            name,
            ..
        } => {
            held = reopen(parent, stamp, climb)?;
            (held.as_fd(), name.as_os_str())
        }
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
    Ok(match kind_at(anchor, name, path, reach)? {
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

pub(super) fn kind_at(
    directory: impl AsFd,
    name: &OsStr,
    path: &Path,
    reach: Reach<'_>,
) -> io::Result<Option<Kind>> {
    Ok(classify(directory, name, path, false, &|| {}, reach)?.map(|observed| observed.kind))
}

pub(super) fn observe(
    directory: impl AsFd,
    name: &OsStr,
    path: &Path,
    resolved: &dyn Fn(),
) -> io::Result<Option<Observed>> {
    classify(directory, name, path, true, resolved, Reach::default())
}

fn modified(stat: &FileStat) -> SystemTime {
    let seconds = stat.st_mtime;
    let nanos = Duration::from_nanos(u64::try_from(stat.st_mtime_nsec).unwrap_or(0));
    match u64::try_from(seconds) {
        Ok(seconds) => SystemTime::UNIX_EPOCH + Duration::from_secs(seconds) + nanos,
        Err(_) => SystemTime::UNIX_EPOCH - Duration::from_secs(seconds.unsigned_abs()) + nanos,
    }
}

fn format_of(stat: &FileStat) -> SFlag {
    SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT
}

fn classify(
    directory: impl AsFd,
    name: &OsStr,
    path: &Path,
    with_modified: bool,
    after_resolve: &dyn Fn(),
    reach: Reach<'_>,
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
        let modified = (format != SFlag::S_IFDIR).then(|| modified(&stat));
        return Ok(Some(Observed { kind, modified }));
    }
    let target = match readlinkat(&directory, name) {
        Ok(target) => PathBuf::from(target),
        Err(nix::errno::Errno::ENOENT) => return Ok(None),
        Err(errno) => return Err(failure(errno)),
    };
    let base = path.parent().unwrap_or(path);
    let followed = match resolve::resolve(directory.as_fd(), base, &target, reach) {
        Ok(followed) => Some(followed),
        Err(error) if unresolved_link(&error) => None,
        Err(error) => return Err(error),
    };
    after_resolve();
    let modified = if with_modified {
        let observed = followed.as_ref().map_or(&stat, |followed| &followed.stat);
        (format_of(observed) != SFlag::S_IFDIR).then(|| modified(observed))
    } else {
        None
    };
    let resolved = followed.map(|followed| followed.path);
    Ok(Some(Observed {
        kind: Kind::Link { target, resolved },
        modified,
    }))
}
