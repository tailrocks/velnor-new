// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::CString;
use std::os::fd::OwnedFd;
use std::path::Path;

use rustix::fs::{
    AtFlags, CWD, Dir, FileType, Mode, OFlags, Stat, fstat, openat, statat, unlinkat,
};
use rustix::io::Errno;
use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};

pub(super) type Identity = (i128, i128);

const DESCRIPTOR_LADDER: [u64; 4] = [1 << 20, 1 << 18, 1 << 16, 10_240];

struct Level {
    directory: OwnedFd,
    pending: Vec<CString>,
    name: Option<CString>,
}

fn directory_flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}

pub(super) fn identity(stat: &Stat) -> Identity {
    (i128::from(stat.st_dev), i128::from(stat.st_ino))
}

fn raise_descriptor_limit() {
    let limit = getrlimit(Resource::Nofile);
    let ceiling = limit.maximum.unwrap_or(u64::MAX);
    for wanted in DESCRIPTOR_LADDER {
        let current = limit.current.unwrap_or(u64::MAX);
        if current >= wanted {
            return;
        }
        let target = wanted.min(ceiling);
        if setrlimit(
            Resource::Nofile,
            Rlimit {
                current: Some(target),
                maximum: limit.maximum,
            },
        )
        .is_ok()
        {
            return;
        }
    }
}

fn enter(
    directory: OwnedFd,
    name: Option<CString>,
    remove: bool,
    visit: &mut impl FnMut(&Stat) -> bool,
) -> Result<Level, Errno> {
    let mut level = Level {
        directory,
        pending: Vec::new(),
        name,
    };
    let mut entries = Dir::read_from(&level.directory)?;
    while let Some(entry) = entries.read() {
        let entry = entry?;
        let name = entry.file_name();
        if matches!(name.to_bytes(), b"." | b"..") {
            continue;
        }
        let stat = match statat(&level.directory, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => stat,
            Err(Errno::NOENT) => continue,
            Err(error) => return Err(error),
        };
        if !visit(&stat) {
            return Err(Errno::INTR);
        }
        if FileType::from_raw_mode(stat.st_mode) == FileType::Directory {
            level.pending.push(name.to_owned());
        } else if remove {
            unlinkat(&level.directory, name, AtFlags::empty())?;
        }
    }
    Ok(level)
}

pub(super) fn walk(
    root: &Path,
    remove: bool,
    mut visit: impl FnMut(&Stat) -> bool,
) -> Result<(), Errno> {
    raise_descriptor_limit();
    let top = match openat(CWD, root, directory_flags(), Mode::empty()) {
        Ok(descriptor) => descriptor,
        Err(Errno::NOENT) => return Ok(()),
        Err(error) => return Err(error),
    };
    if !visit(&fstat(&top)?) {
        return Err(Errno::INTR);
    }
    let mut ancestors: Vec<Level> = Vec::new();
    let mut level = enter(top, None, remove, &mut visit)?;
    loop {
        if let Some(name) = level.pending.pop() {
            let child = openat(
                &level.directory,
                name.as_c_str(),
                directory_flags(),
                Mode::empty(),
            )?;
            let entered = enter(child, Some(name), remove, &mut visit)?;
            ancestors.push(std::mem::replace(&mut level, entered));
            continue;
        }
        let Some(parent) = ancestors.pop() else {
            return Ok(());
        };
        let left = std::mem::replace(&mut level, parent);
        if remove && let Some(name) = left.name {
            unlinkat(&level.directory, name.as_c_str(), AtFlags::REMOVEDIR)?;
        }
    }
}
