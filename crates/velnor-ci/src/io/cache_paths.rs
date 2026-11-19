// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Component, Path, PathBuf};

use super::inputs::tracked_paths;
use crate::error::CiError;

pub(super) fn validated(root: &Path, target: &Path) -> Result<PathBuf, CiError> {
    let listed = tracked_paths(root)?;

    let mut normalized = PathBuf::new();
    for part in root.join(target).components() {
        match part {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            other => normalized.push(other),
        }
    }
    let relative = normalized
        .strip_prefix(&listed.top)
        .map_err(|_| CiError::new("cache target is outside the checkout"))?;
    let tracked = std::process::Command::new("git")
        .args(["ls-files", "-z", "--"])
        .arg(relative)
        .current_dir(&listed.top)
        .output()
        .map_err(|e| CiError::new(e.to_string()))?;
    if !tracked.status.success() || !tracked.stdout.is_empty() {
        return Err(CiError::new("cache target contains tracked content"));
    }
    let forbidden = relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| part.as_os_str() == ".git" || part.as_os_str() == ".cargo")
        || listed.paths.iter().any(|path| {
            let path = Path::new(path);
            path.starts_with(relative) || relative.starts_with(path)
        });
    if forbidden {
        return Err(CiError::new(format!(
            "unsafe cache target {} overlaps checkout content",
            target.display()
        )));
    }
    let mut path = listed.top;
    for component in relative.components() {
        path.push(component);
        match fs::symlink_metadata(&path) {
            Ok(meta) if !meta.is_dir() => {
                return Err(CiError::new(format!(
                    "cache target {} is not a regular directory",
                    path.display()
                )));
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(CiError::new(format!(
                    "cannot inspect {}: {error}",
                    path.display()
                )));
            }
            _ => {}
        }
    }
    Ok(normalized)
}

pub(super) fn list(name: &str) -> Result<Option<Vec<PathBuf>>, CiError> {
    match std::env::var(name) {
        Ok(text) if !text.is_empty() => Ok(Some(text.lines().map(PathBuf::from).collect())),
        Err(std::env::VarError::NotPresent) => Ok(None),
        _ => Err(CiError::new(format!(
            "{name} must hold a nonempty list of paths"
        ))),
    }
}
