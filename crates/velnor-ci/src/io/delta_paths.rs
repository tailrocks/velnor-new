// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Component, Path, PathBuf};

use super::delta::{failure, safe_path};
use crate::error::CiError;

pub(super) fn staging(path: &Path) -> Result<(), CiError> {
    safe_path(path)?;
    untracked(path, "staging")
}

pub(super) fn untracked(path: &Path, role: &str) -> Result<(), CiError> {
    let root = std::env::current_dir().map_err(|error| CiError::new(error.to_string()))?;
    let tracked = super::inputs::tracked_paths(&root)?;
    let destination = resolve(path)?;
    for path in tracked.paths {
        let full = tracked.top.join(path);
        let parent = full
            .parent()
            .ok_or_else(|| CiError::new("tracked path has no parent"))?;
        let tracked = resolve(parent)?.join(
            full.file_name()
                .ok_or_else(|| CiError::new("tracked path has no filename"))?,
        );
        if tracked.starts_with(&destination) || destination.starts_with(&tracked) {
            return Err(CiError::new(format!(
                "delta {role} overlaps tracked input {}",
                tracked.display()
            )));
        }
    }
    Ok(())
}

fn resolve(path: &Path) -> Result<PathBuf, CiError> {
    let absolute = std::path::absolute(path).map_err(|error| failure(path, &error))?;
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        if component == Component::ParentDir {
            normalized.pop();
        } else if component != Component::CurDir {
            normalized.push(component);
        }
    }
    let mut ancestor = normalized.as_path();
    let mut suffix = Vec::new();
    loop {
        match fs::canonicalize(ancestor) {
            Ok(mut canonical) => {
                for component in suffix.into_iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                let name = ancestor
                    .file_name()
                    .ok_or_else(|| failure(ancestor, &error))?;
                suffix.push(name);
                ancestor = ancestor.parent().ok_or_else(|| failure(ancestor, &error))?;
            }
            Err(error) => return Err(failure(ancestor, &error)),
        }
    }
}
