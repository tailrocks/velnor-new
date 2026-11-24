// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

pub(super) type Identity = (u64, u64, i64, i64);

pub(super) fn identity(metadata: &fs::Metadata) -> Identity {
    (
        metadata.dev(),
        metadata.ino(),
        metadata.mtime(),
        metadata.mtime_nsec(),
    )
}

pub(super) fn open_entry(entry: &Path) -> std::io::Result<File> {
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(entry)
}

pub(super) fn published(held: &File, entry: &Path) -> bool {
    match (held.metadata(), fs::symlink_metadata(entry)) {
        (Ok(held), Ok(named)) => identity(&held) == identity(&named),
        _ => false,
    }
}
