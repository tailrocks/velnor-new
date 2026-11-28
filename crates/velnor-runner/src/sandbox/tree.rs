// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::CString;
use std::os::fd::OwnedFd;
use std::path::Path;

use rustix::fs::{AtFlags, CWD, Dir, FileType, Mode, OFlags, Stat, fstat, openat, statat};

pub(super) type Identity = (i128, i128);

pub(super) const MAX_DEPTH: usize = 1024;

struct Level {
    directory: OwnedFd,
    pending: Vec<CString>,
}

pub(super) fn directory_flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}

pub(super) fn identity(stat: &Stat) -> Identity {
    (i128::from(stat.st_dev), i128::from(stat.st_ino))
}

fn enter(directory: OwnedFd, visit: &mut impl FnMut(&Stat) -> bool) -> Option<Level> {
    let mut pending = Vec::new();
    let mut entries = Dir::read_from(&directory).ok()?;
    while let Some(Ok(entry)) = entries.read() {
        let name = entry.file_name();
        if matches!(name.to_bytes(), b"." | b"..") {
            continue;
        }
        let Ok(stat) = statat(&directory, name, AtFlags::SYMLINK_NOFOLLOW) else {
            continue;
        };
        if !visit(&stat) {
            return None;
        }
        if FileType::from_raw_mode(stat.st_mode) == FileType::Directory {
            pending.push(name.to_owned());
        }
    }
    Some(Level { directory, pending })
}

pub(super) fn walk(root: &Path, max_depth: usize, mut visit: impl FnMut(&Stat) -> bool) {
    let Ok(top) = openat(CWD, root, directory_flags(), Mode::empty()) else {
        return;
    };
    let Ok(stat) = fstat(&top) else {
        return;
    };
    if !visit(&stat) {
        return;
    }
    let Some(first) = enter(top, &mut visit) else {
        return;
    };
    let mut ancestors: Vec<Level> = Vec::new();
    let mut level = first;
    loop {
        if ancestors.len() + 1 < max_depth
            && let Some(name) = level.pending.pop()
        {
            let entered = openat(
                &level.directory,
                name.as_c_str(),
                directory_flags(),
                Mode::empty(),
            )
            .ok()
            .and_then(|child| enter(child, &mut visit));
            if let Some(entered) = entered {
                ancestors.push(std::mem::replace(&mut level, entered));
            }
            continue;
        }
        let Some(parent) = ancestors.pop() else {
            return;
        };
        level = parent;
    }
}
