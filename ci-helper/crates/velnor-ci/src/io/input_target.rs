// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::inputs::tracked_paths;
use crate::error::CiError;

pub(super) fn check_target(root: &Path, target: &Path) -> Result<(), CiError> {
    let target = resolve(&root.join(target))?;
    let root = resolve(root)?;
    let tracked = tracked_paths(&root)?;
    if tracked.non_utf8 > 0 {
        return Err(CiError::new(
            "cannot verify target directory: tracked paths are not valid UTF-8",
        ));
    }
    let metadata = [
        resolve(&tracked.top.join(".git"))?,
        git_path(&root, "--absolute-git-dir")?,
        git_path(&root, "--git-common-dir")?,
    ];
    if root.starts_with(&target)
        || metadata
            .iter()
            .any(|path| path.starts_with(&target) || target.starts_with(path))
        || tracked
            .paths
            .iter()
            .any(|path| tracked.top.join(path).starts_with(&target))
    {
        return Err(CiError::new(format!(
            "target directory {} contains checkout files",
            target.display()
        )));
    }
    Ok(())
}

fn git_path(root: &Path, option: &str) -> Result<PathBuf, CiError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--path-format=absolute", option])
        .output()
        .map_err(|e| CiError::new(format!("cannot run git rev-parse: {e}")))?;
    if !output.status.success() {
        return Err(CiError::new(format!(
            "git rev-parse {option} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let path = String::from_utf8(output.stdout)
        .map_err(|e| CiError::new(format!("git rev-parse printed non-UTF-8: {e}")))?;
    resolve(Path::new(path.trim_end_matches(['\n', '\r'])))
}

fn resolve(path: &Path) -> Result<PathBuf, CiError> {
    match fs::canonicalize(path) {
        Ok(path) => Ok(path),
        Err(e) if e.kind() == ErrorKind::NotFound => {
            let parent = path.parent().zip(path.file_name()).ok_or_else(|| {
                CiError::new(format!(
                    "cannot resolve target directory {}: {e}",
                    path.display()
                ))
            })?;
            Ok(resolve(parent.0)?.join(parent.1))
        }
        Err(e) => Err(CiError::new(format!(
            "cannot resolve target directory {}: {e}",
            path.display()
        ))),
    }
}
