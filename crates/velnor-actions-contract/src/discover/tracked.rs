// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use super::index::{FileIndex, IndexError, build_index_from_list};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexMode {
    Tracked,

    Untracked,

    Both,
}

pub fn build_index_from_tracked(
    root: &Path,
    tracked: &[String],
    untracked: &[String],
    mode: IndexMode,
    exclusions: &[String],
) -> Result<FileIndex, IndexError> {
    let mut files = Vec::new();
    if mode != IndexMode::Untracked {
        files.extend(tracked.iter().cloned());
    }
    if mode != IndexMode::Tracked {
        files.extend(untracked.iter().cloned());
    }
    build_index_from_list(root, &files, exclusions)
}
