// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use super::inputs::{modified_ns, set_modified_ns, tracked_paths};
use crate::error::CiError;
use crate::ties::break_ties;

pub fn break_time_ties(root: &Path) -> Result<String, CiError> {
    let (top, paths) = tracked_paths(root)?;
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        let full = top.join(&path);
        match fs::symlink_metadata(&full) {
            Ok(metadata) if metadata.is_file() => files.push((path, modified_ns(&full)?)),
            _ => {}
        }
    }
    let moved = break_ties(&files);
    for (path, time) in &moved {
        set_modified_ns(&top.join(path), *time)?;
    }
    Ok(format!(
        "gave {} tracked files a modification time no other tracked file has, out of {}\n",
        moved.len(),
        files.len()
    ))
}
