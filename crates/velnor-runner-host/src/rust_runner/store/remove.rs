// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::CString;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

use rustix::fs::{
    AtFlags, Dir, FileType, Mode, Stat, chmodat, fchmod, fstat, openat, statat, unlinkat,
};
use rustix::io::Errno;
use rustix::path::Arg;

use super::{directory_flags, remove_entry};
use crate::error::RustRunnerFailure;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy)]
pub(super) struct Bounds {
    pub(super) depth: usize,
    pub(super) entries: u64,
}

pub(super) const BOUNDS: Bounds = Bounds {
    depth: 10_000,
    entries: 10_000_000,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Removal {
    Os(Errno),
    Moved,
    TooDeep,
    TooMany,
}

impl From<Errno> for Removal {
    fn from(error: Errno) -> Self {
        Self::Os(error)
    }
}

impl Removal {
    pub(super) const fn failure(self) -> RustRunnerFailure {
        match self {
            Self::Os(_) | Self::Moved => RustRunnerFailure::HostStore,
            Self::TooDeep | Self::TooMany => RustRunnerFailure::WorkTreeBound,
        }
    }
}

struct Frame {
    name: CString,
    parent: Stat,
}

pub(super) fn tree(
    directory: &impl AsFd,
    name: impl Arg + Copy,
    bounds: Bounds,
) -> Result<(), Removal> {
    let stat = match statat(directory, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(Errno::NOENT) => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory {
        return Ok(remove_entry(directory, name)?);
    }
    let child = open_directory(directory, name)?;
    contents(&child, bounds)?;
    Ok(unlinkat(directory, name, AtFlags::REMOVEDIR)?)
}

pub(super) fn contents(root: &impl AsFd, bounds: Bounds) -> Result<(), Removal> {
    let mut path: Vec<Frame> = Vec::new();
    let mut current: Option<OwnedFd> = None;
    let mut seen = 0_u64;
    loop {
        let here = current.as_ref().map_or_else(|| root.as_fd(), AsFd::as_fd);
        if let Some(name) = drain(here, &mut seen, bounds.entries)? {
            if path.len() >= bounds.depth {
                return Err(Removal::TooDeep);
            }
            let parent = fstat(here)?;
            let child = open_directory(&here, name.as_c_str())?;
            path.push(Frame { name, parent });
            current = Some(child);
        } else {
            let Some(frame) = path.pop() else {
                return Ok(());
            };
            let parent = ascend(here, &frame.parent)?;
            unlinkat(&parent, frame.name.as_c_str(), AtFlags::REMOVEDIR)?;
            current = (!path.is_empty()).then_some(parent);
        }
    }
}

fn ascend(here: BorrowedFd<'_>, expected: &Stat) -> Result<OwnedFd, Removal> {
    let parent = openat(here, c"..", directory_flags(), Mode::empty())?;
    let found = fstat(&parent)?;
    if (found.st_dev, found.st_ino) == (expected.st_dev, expected.st_ino) {
        Ok(parent)
    } else {
        Err(Removal::Moved)
    }
}

fn drain(
    directory: BorrowedFd<'_>,
    seen: &mut u64,
    limit: u64,
) -> Result<Option<CString>, Removal> {
    for entry in Dir::read_from(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        if matches!(name.to_bytes(), b"." | b"..") {
            continue;
        }
        *seen += 1;
        if *seen > limit {
            return Err(Removal::TooMany);
        }
        let kind = match entry.file_type() {
            FileType::Unknown => {
                FileType::from_raw_mode(statat(directory, name, AtFlags::SYMLINK_NOFOLLOW)?.st_mode)
            }
            known => known,
        };
        if kind == FileType::Directory {
            return Ok(Some(name.to_owned()));
        }
        remove_entry(&directory, name)?;
    }
    Ok(None)
}

fn open_directory(parent: &impl AsFd, name: impl Arg + Copy) -> Result<OwnedFd, Errno> {
    let child = match openat(parent, name, directory_flags(), Mode::empty()) {
        Err(Errno::ACCESS) => {
            chmodat(parent, name, Mode::RWXU, AtFlags::empty())?;
            openat(parent, name, directory_flags(), Mode::empty())?
        }
        opened => opened?,
    };
    fchmod(&child, Mode::RWXU)?;
    Ok(child)
}
