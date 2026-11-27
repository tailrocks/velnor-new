// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use anyhow::{Context, Result};
use rustix::fs::{
    AtFlags, CWD, Dir, FileType, Mode, OFlags, RawMode, fchmod, fstat, mkdirat, openat, renameat,
    unlinkat,
};
use rustix::io::Errno;
use rustix::path::Arg;

const OPEN_LEVELS: usize = 32;

pub(crate) fn directory_flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}

pub(crate) fn open_path(path: &Path) -> rustix::io::Result<OwnedFd> {
    openat(
        CWD,
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )
}

pub(crate) fn enter_directory_as(
    parent: BorrowedFd<'_>,
    name: &OsStr,
    bits: RawMode,
) -> rustix::io::Result<(OwnedFd, bool)> {
    match openat(parent, name, directory_flags(), Mode::empty()) {
        Err(Errno::NOENT) => {
            let created = match mkdirat(parent, name, Mode::from_bits_truncate(bits)) {
                Ok(()) => true,
                Err(Errno::EXIST) => false,
                Err(error) => return Err(error),
            };
            Ok((
                openat(parent, name, directory_flags(), Mode::empty())?,
                created,
            ))
        }
        other => other.map(|directory| (directory, false)),
    }
}

pub(crate) fn enter_directory(parent: BorrowedFd<'_>, name: &OsStr) -> rustix::io::Result<OwnedFd> {
    enter_directory_as(parent, name, 0o777).map(|(directory, _)| directory)
}

pub(crate) fn is_regular_mode(mode: RawMode) -> bool {
    FileType::from_raw_mode(mode) == FileType::RegularFile
}

pub(crate) fn open_regular<P: Arg>(parent: BorrowedFd<'_>, path: P) -> io::Result<File> {
    let fd = openat(
        parent,
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let kind = fstat(&fd)?;
    if is_regular_mode(kind.st_mode) {
        Ok(File::from(fd))
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ))
    }
}

fn grant_owner(directory: &OwnedFd) -> rustix::io::Result<()> {
    let mode = fstat(directory)?.st_mode;
    fchmod(directory, Mode::from_bits_truncate(mode | 0o700))
}

fn rename_aside(
    from: BorrowedFd<'_>,
    name: &OsStr,
    aside: BorrowedFd<'_>,
) -> Result<Option<String>> {
    let target = uuid::Uuid::new_v4().to_string();
    match renameat(from, name, aside, target.as_str()) {
        Ok(()) => Ok(Some(target)),
        Err(Errno::NOENT) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn move_aside(from: BorrowedFd<'_>, name: &OsStr, aside: BorrowedFd<'_>) -> Result<()> {
    rename_aside(from, name, aside).map(drop)
}

pub(crate) fn remove_entry(
    from: BorrowedFd<'_>,
    name: &OsStr,
    aside: BorrowedFd<'_>,
) -> Result<()> {
    if let Ok(entry) = openat(from, name, directory_flags(), Mode::empty()) {
        grant_owner(&entry).context("make the cache entry movable")?;
    }
    match rename_aside(from, name, aside)? {
        Some(target) => purge(aside, OsStr::new(&target)),
        None => Ok(()),
    }
}

struct Level {
    fd: OwnedFd,
    name: OsString,
    children: Vec<OsString>,
}

enum Step {
    Gone,
    Level(Level),
    Hoisted(OsString),
}

pub(crate) fn names(fd: &OwnedFd) -> Result<Vec<OsString>> {
    let mut out = Vec::new();
    for item in Dir::read_from(fd)? {
        let name = OsStr::from_bytes(item?.file_name().to_bytes()).to_owned();
        if name != "." && name != ".." {
            out.push(name);
        }
    }
    Ok(out)
}

fn descend(parent: BorrowedFd<'_>, name: &OsStr, root: BorrowedFd<'_>, deep: bool) -> Result<Step> {
    match openat(parent, name, directory_flags(), Mode::empty()) {
        Ok(_) if deep => {
            let hoisted = OsString::from(format!(".purge-{}", uuid::Uuid::new_v4()));
            renameat(parent, name, root, hoisted.as_os_str())?;
            Ok(Step::Hoisted(hoisted))
        }
        Ok(fd) => {
            let _ = grant_owner(&fd);
            let children = names(&fd)?;
            Ok(Step::Level(Level {
                fd,
                name: name.to_owned(),
                children,
            }))
        }
        Err(Errno::NOENT) => Ok(Step::Gone),
        Err(Errno::LOOP | Errno::NOTDIR) => {
            match unlinkat(parent, name, AtFlags::empty()) {
                Ok(()) | Err(Errno::NOENT) => {}
                Err(error) => return Err(error.into()),
            }
            Ok(Step::Gone)
        }
        Err(error) => Err(error.into()),
    }
}

fn purge_one(root: BorrowedFd<'_>, name: &OsStr, pending: &mut Vec<OsString>) -> Result<()> {
    let mut stack = Vec::new();
    match descend(root, name, root, false)? {
        Step::Level(level) => stack.push(level),
        Step::Gone | Step::Hoisted(_) => return Ok(()),
    }
    while let Some(top) = stack.len().checked_sub(1) {
        let Some(child) = stack[top].children.pop() else {
            if let Some(done) = stack.pop() {
                let parent = stack.last().map_or(root, |level| level.fd.as_fd());
                unlinkat(parent, &done.name, AtFlags::REMOVEDIR)?;
            }
            continue;
        };
        let deep = stack.len() >= OPEN_LEVELS;
        match descend(stack[top].fd.as_fd(), &child, root, deep)? {
            Step::Level(level) => stack.push(level),
            Step::Hoisted(hoisted) => pending.push(hoisted),
            Step::Gone => {}
        }
    }
    Ok(())
}

pub(crate) fn purge(parent: BorrowedFd<'_>, name: &OsStr) -> Result<()> {
    let Step::Level(Level { fd, children, .. }) = descend(parent, name, parent, false)? else {
        return Ok(());
    };
    let mut pending = children;
    while let Some(next) = pending.pop() {
        purge_one(fd.as_fd(), &next, &mut pending)?;
    }
    unlinkat(parent, name, AtFlags::REMOVEDIR)?;
    Ok(())
}
