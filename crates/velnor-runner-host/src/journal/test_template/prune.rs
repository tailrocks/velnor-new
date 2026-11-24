// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::os::fd::OwnedFd;
use std::path::Path;

use rustix::fs::{AtFlags, Dir, FileType, Mode, OFlags, openat, statat, unlinkat};

fn flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}

pub(super) fn prune_except(root: &Path, keep: &str) -> io::Result<()> {
    let anchor = rustix::fs::open(root, flags(), Mode::empty())?;
    remove_children(&anchor, Some(keep))
}

fn remove_children(directory: &OwnedFd, keep: Option<&str>) -> io::Result<()> {
    for entry in Dir::read_from(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        if matches!(name.to_bytes(), b"." | b"..")
            || keep.is_some_and(|keep| name.to_bytes() == keep.as_bytes())
        {
            continue;
        }
        let is_directory = statat(directory, name, AtFlags::SYMLINK_NOFOLLOW)
            .is_ok_and(|stat| FileType::from_raw_mode(stat.st_mode) == FileType::Directory);
        if is_directory {
            if let Ok(child) = openat(directory, name, flags(), Mode::empty()) {
                remove_children(&child, None).ok();
            }
            unlinkat(directory, name, AtFlags::REMOVEDIR).ok();
        } else {
            unlinkat(directory, name, AtFlags::empty()).ok();
        }
    }
    Ok(())
}
