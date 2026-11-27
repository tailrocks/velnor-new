// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use rustix::fs::{Mode, OFlags, openat};
use rustix::io::Errno;
use std::ffi::{OsStr, OsString};
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::rc::Rc;

pub(super) const READ_ENTRY: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);
pub(super) const READ_DIRECTORY: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

const CHECKPOINTS_PER_DOUBLING_BITS: u32 = 4;

#[derive(Clone)]
pub(super) struct Anchor {
    fd: Rc<OwnedFd>,
    level: usize,
    below: Rc<Vec<OsString>>,
}

pub(super) fn open_directory_below<'a>(
    root: &Rc<OwnedFd>,
    names: impl IntoIterator<Item = &'a OsStr>,
) -> Result<Rc<OwnedFd>, (Errno, PathBuf)> {
    let mut current = Rc::clone(root);
    let mut walked = PathBuf::new();
    for name in names {
        walked.push(name);
        let next = openat(&*current, name, READ_DIRECTORY, Mode::empty())
            .map_err(|error| (error, walked.clone()))?;
        current = Rc::new(next);
    }
    Ok(current)
}

impl Anchor {
    pub(super) fn root(fd: Rc<OwnedFd>) -> Self {
        Self {
            fd,
            level: 0,
            below: Rc::default(),
        }
    }

    pub(super) fn parent(&self) -> Result<Rc<OwnedFd>, (Errno, PathBuf)> {
        open_directory_below(&self.fd, self.below.iter().map(OsString::as_os_str))
    }

    pub(super) fn descend(&self, name: &OsStr, directory: OwnedFd) -> Self {
        let level = self.level + self.below.len() + 1;
        if is_checkpoint(level) {
            return Self {
                fd: Rc::new(directory),
                level,
                below: Rc::default(),
            };
        }
        let mut below = Vec::with_capacity(self.below.len() + 1);
        below.extend(self.below.iter().cloned());
        below.push(name.to_owned());
        Self {
            fd: Rc::clone(&self.fd),
            level: self.level,
            below: Rc::new(below),
        }
    }
}

fn is_checkpoint(level: usize) -> bool {
    let doublings = usize::BITS - 1 - level.leading_zeros();
    let spacing = 1_usize << doublings.saturating_sub(CHECKPOINTS_PER_DOUBLING_BITS);
    level.is_multiple_of(spacing)
}
