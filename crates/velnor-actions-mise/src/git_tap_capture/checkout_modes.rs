// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    os::fd::{AsFd, OwnedFd},
    path::Path,
};

use rustix::fs::{self, FileType, Mode, OFlags};

use super::{TapTreeEntry, invalid};
use crate::CheckDeadline;

const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
const FILE_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::NONBLOCK)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

pub(super) fn normalize(
    checkout: &Path,
    entries: &[TapTreeEntry],
    deadline: CheckDeadline,
) -> Result<(), crate::MiseError> {
    let root = fs::open(checkout, DIRECTORY_FLAGS, Mode::empty())
        .map_err(|_| invalid("tap_checkout_root"))?;
    for entry in entries {
        deadline.remaining()?;
        if entry
            .path
            .split('/')
            .any(|part| part.eq_ignore_ascii_case(".git"))
            || !matches!(entry.mode, 0o100_644 | 0o100_755)
        {
            return Err(invalid("tap_checkout_mode_entry"));
        }
    }
    for entry in entries {
        deadline.remaining()?;
        ensure_regular(&open_leaf(&root, &entry.path)?)?;
    }
    for entry in entries {
        deadline.remaining()?;
        let file = open_leaf(&root, &entry.path)?;
        ensure_regular(&file)?;
        let mode = match entry.mode {
            0o100_644 => 0o644,
            0o100_755 => 0o755,
            _ => return Err(invalid("tap_checkout_mode_entry")),
        };
        fs::fchmod(&file, Mode::from_raw_mode(mode)).map_err(|_| invalid("tap_checkout_chmod"))?;
    }
    Ok(())
}

fn open_leaf(root: &OwnedFd, path: &str) -> Result<OwnedFd, crate::MiseError> {
    let mut parent: Option<OwnedFd> = None;
    let mut components = path.split('/').peekable();
    while let Some(component) = components.next() {
        let directory = parent
            .as_ref()
            .map_or_else(|| root.as_fd(), |directory| directory.as_fd());
        if components.peek().is_some() {
            parent = Some(
                fs::openat(directory, component, DIRECTORY_FLAGS, Mode::empty())
                    .map_err(|_| invalid("tap_checkout_directory"))?,
            );
        } else {
            return fs::openat(directory, component, FILE_FLAGS, Mode::empty())
                .map_err(|_| invalid("tap_checkout_file"));
        }
    }
    Err(invalid("tap_checkout_file"))
}

fn ensure_regular(file: &OwnedFd) -> Result<(), crate::MiseError> {
    let metadata = fs::fstat(file).map_err(|_| invalid("tap_checkout_file"))?;
    if FileType::from_raw_mode(metadata.st_mode) != FileType::RegularFile {
        return Err(invalid("tap_checkout_nonregular_file"));
    }
    if metadata.st_nlink != 1 {
        return Err(invalid("tap_checkout_hardlinked_file"));
    }
    Ok(())
}
