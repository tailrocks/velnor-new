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
    let mut parent = None;
    for entry in entries {
        deadline.remaining()?;
        let file = open_leaf(&root, &mut parent, &entry.path, deadline)?;
        let observed_mode = ensure_regular(&file)?;
        let mode = match entry.mode {
            0o100_644 => 0o644,
            0o100_755 => 0o755,
            _ => return Err(invalid("tap_checkout_mode_entry")),
        };
        let expected_mode = Mode::from_raw_mode(mode);
        if observed_mode != expected_mode {
            fs::fchmod(&file, expected_mode).map_err(|_| invalid("tap_checkout_chmod"))?;
        }
    }
    Ok(())
}

fn open_leaf(
    root: &OwnedFd,
    cached: &mut Option<(String, OwnedFd)>,
    path: &str,
    deadline: CheckDeadline,
) -> Result<OwnedFd, crate::MiseError> {
    let (parent_path, leaf) = path.rsplit_once('/').unwrap_or(("", path));
    if cached.as_ref().is_none_or(|(path, _)| path != parent_path) {
        *cached = None;
        if !parent_path.is_empty() {
            let mut parent: Option<OwnedFd> = None;
            for component in parent_path.split('/') {
                deadline.remaining()?;
                let directory = parent
                    .as_ref()
                    .map_or_else(|| root.as_fd(), |directory| directory.as_fd());
                parent = Some(
                    fs::openat(directory, component, DIRECTORY_FLAGS, Mode::empty())
                        .map_err(|_| invalid("tap_checkout_directory"))?,
                );
            }
            *cached = parent.map(|directory| (parent_path.to_owned(), directory));
        }
    }
    deadline.remaining()?;
    let directory = cached
        .as_ref()
        .map_or_else(|| root.as_fd(), |(_, directory)| directory.as_fd());
    fs::openat(directory, leaf, FILE_FLAGS, Mode::empty()).map_err(|_| invalid("tap_checkout_file"))
}

fn ensure_regular(file: &OwnedFd) -> Result<Mode, crate::MiseError> {
    let metadata = fs::fstat(file).map_err(|_| invalid("tap_checkout_file"))?;
    if FileType::from_raw_mode(metadata.st_mode) != FileType::RegularFile {
        return Err(invalid("tap_checkout_nonregular_file"));
    }
    if metadata.st_nlink != 1 {
        return Err(invalid("tap_checkout_hardlinked_file"));
    }
    Ok(Mode::from_raw_mode(metadata.st_mode))
}
