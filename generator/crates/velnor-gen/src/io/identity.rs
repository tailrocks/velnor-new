// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::path::Path;

use super::program;

pub(super) fn repository_name(root: &Path) -> String {
    let remote = program(
        root,
        "git",
        &[
            OsStr::new("config"),
            OsStr::new("--get"),
            OsStr::new("remote.origin.url"),
        ],
    )
    .ok()
    .and_then(|bytes| String::from_utf8(bytes).ok());
    let from_remote = remote.as_deref().and_then(last_segment);
    let from_folder = root.file_name().and_then(OsStr::to_str);
    from_remote.or(from_folder).unwrap_or_default().to_owned()
}

fn last_segment(url: &str) -> Option<&str> {
    let trimmed = url.trim().trim_end_matches('/');
    let name = trimmed.rsplit(['/', ':']).next()?;
    let name = name.strip_suffix(".git").unwrap_or(name);
    (!name.is_empty()).then_some(name)
}
