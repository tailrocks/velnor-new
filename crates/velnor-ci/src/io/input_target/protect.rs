// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use super::{git_marker, metadata};
use crate::error::CiError;

/// Walks start over this many times before the check gives up.
const PROTECT_ATTEMPTS: usize = 8;

/// Refuses when a Git checkout sits under `target`, because removal would delete it.
///
/// Concurrent cargo lanes create and remove directories while this walk runs. A directory that
/// vanishes after its parent listed it may have been renamed, and a checkout moved into a part of
/// the tree this walk has not reached yet would then pass. So a walk that sees a vanished
/// directory starts again, and the check passes only when one complete walk sees none.
pub(super) fn protect_tree(target: &Path) -> Result<(), CiError> {
    protect_tree_with(target, &mut |_| {})
}

/// `before_read` runs just before each directory is read; tests use it to change the tree there.
fn protect_tree_with(target: &Path, before_read: &mut dyn FnMut(&Path)) -> Result<(), CiError> {
    for _ in 0..PROTECT_ATTEMPTS {
        if protect_walk(target, before_read)? {
            return Ok(());
        }
    }
    Err(CiError::new(format!(
        "cannot inspect {}: its directories kept changing during the check",
        target.display()
    )))
}

/// One walk. Returns `Ok(false)` when a listed directory vanished before it could be read.
fn protect_walk(target: &Path, before_read: &mut dyn FnMut(&Path)) -> Result<bool, CiError> {
    let mut pending = vec![target.to_path_buf()];
    while let Some(path) = pending.pop() {
        let Some(found) = metadata(&path)? else {
            if path == target {
                continue;
            }
            return Ok(false);
        };
        if !found.is_dir() {
            continue;
        }
        if git_marker(&path)? {
            return Err(CiError::new(format!(
                "target directory {} contains Git checkout {}",
                target.display(),
                path.display()
            )));
        }
        before_read(&path);
        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(false),
            Err(e) => {
                return Err(CiError::new(format!(
                    "cannot inspect {}: {e}",
                    path.display()
                )));
            }
        };
        for entry in entries {
            let entry = entry
                .map_err(|e| CiError::new(format!("cannot inspect {}: {e}", path.display())))?;
            let kind = entry.file_type().map_err(|e| {
                CiError::new(format!("cannot inspect {}: {e}", entry.path().display()))
            })?;
            if kind.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
#[path = "protect_tests.rs"]
mod tests;
