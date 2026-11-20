// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::inputs::{TrackedPaths, modified_ns, set_modified_ns, tracked_paths};
use crate::error::CiError;
use crate::ties::break_ties;

pub fn break_time_ties(root: &Path) -> Result<String, CiError> {
    break_time_ties_with(root, &set_modified_ns)
}

pub fn break_time_ties_with(
    root: &Path,
    set: &dyn Fn(&Path, u64) -> Result<(), CiError>,
) -> Result<String, CiError> {
    let TrackedPaths { top, paths } = tracked_paths(root)?;
    let non_utf8 = paths.iter().filter(|path| path.to_str().is_none()).count();
    if non_utf8 > 0 {
        return Err(CiError::new(format!(
            "{non_utf8} tracked paths are not valid UTF-8 and cannot be tie-broken, so the cache is not used"
        )));
    }
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        let full = top.join(&path);
        let path = path
            .to_str()
            .ok_or_else(|| CiError::new("tracked path is not valid UTF-8"))?
            .to_owned();
        match fs::symlink_metadata(&full) {
            Ok(metadata) if metadata.is_file() => files.push((path, modified_ns(&full)?)),
            _ => {}
        }
    }
    let moved = break_ties(&files)?;
    for (path, time) in &moved {
        set(&top.join(path), *time)?;
    }
    if !moved.is_empty() {
        let mut seen: BTreeMap<u64, &str> = BTreeMap::new();
        for (path, _) in &files {
            let time = modified_ns(&top.join(path))?;
            if let Some(other) = seen.insert(time, path) {
                return Err(CiError::new(format!(
                    "{other} and {path} still share a modification time ({time} ns) after tie breaking; the filesystem cannot hold times this close together, so the cache is not used"
                )));
            }
        }
    }
    Ok(format!(
        "gave {} tracked files a modification time no other tracked file has, out of {}\n",
        moved.len(),
        files.len()
    ))
}
