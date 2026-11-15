// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::Path;

use velnor_model::RepoPath;

use super::{Error, program};

pub(super) fn tracked(root: &Path) -> Result<BTreeSet<RepoPath>, Error> {
    program(root, "git", &[OsStr::new("ls-files"), OsStr::new("-z")])?
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let text = std::str::from_utf8(entry).map_err(|_| {
                Error::Repository(format!(
                    "tracked path {:?} is not UTF-8",
                    String::from_utf8_lossy(entry)
                ))
            })?;
            RepoPath::new(text).map_err(|e| Error::Repository(e.to_string()))
        })
        .collect()
}
