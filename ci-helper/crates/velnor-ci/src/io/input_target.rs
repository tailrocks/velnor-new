// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::inputs::tracked_paths;
use crate::error::CiError;

pub(super) fn check_target(root: &Path, target: &Path) -> Result<(), CiError> {
    let path = root.join(target);
    let object = target_object(&path)?;
    let mut targets = vec![object, resolve(&path)?];
    targets.dedup();
    let top = protect_checkout(root, &targets)?;
    let mut checked = BTreeSet::from([top]);
    for target in &targets {
        for ancestor in target.ancestors() {
            if !checked.contains(ancestor) && git_marker(ancestor)? {
                protect_checkout(ancestor, &targets)?;
                checked.insert(ancestor.to_path_buf());
            }
        }
        protect_tree(target)?;
    }
    Ok(())
}

pub(super) fn target_object(path: &Path) -> Result<PathBuf, CiError> {
    match path.parent().zip(path.file_name()) {
        Some((parent, name)) => Ok(resolve(parent)?.join(name)),
        None => resolve(path),
    }
}

fn protect_checkout(root: &Path, targets: &[PathBuf]) -> Result<PathBuf, CiError> {
    let root = resolve(root)?;
    let tracked = tracked_paths(&root)?;
    let top = resolve(&tracked.top)?;
    let metadata = [
        resolve(&top.join(".git"))?,
        git_path(&root, "--absolute-git-dir")?,
        git_path(&root, "--git-common-dir")?,
    ];
    if targets.iter().any(|target| {
        root.starts_with(target)
            || metadata
                .iter()
                .any(|path| path.starts_with(target) || target.starts_with(path))
            || tracked
                .paths
                .iter()
                .any(|path| top.join(path).starts_with(target))
    }) {
        return Err(CiError::new(format!(
            "target directory {} contains checkout files",
            targets[0].display()
        )));
    }
    Ok(top)
}

fn protect_tree(target: &Path) -> Result<(), CiError> {
    let mut pending = vec![target.to_path_buf()];
    while let Some(path) = pending.pop() {
        if !metadata(&path)?.is_some_and(|metadata| metadata.is_dir()) {
            continue;
        }
        if git_marker(&path)? {
            return Err(CiError::new(format!(
                "target directory {} contains Git checkout {}",
                target.display(),
                path.display()
            )));
        }
        for entry in fs::read_dir(&path)
            .map_err(|e| CiError::new(format!("cannot inspect {}: {e}", path.display())))?
        {
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
    Ok(())
}

fn git_marker(path: &Path) -> Result<bool, CiError> {
    Ok(metadata(&path.join(".git"))?.is_some()
        || (metadata(&path.join("HEAD"))?.is_some_and(|metadata| metadata.is_file())
            && metadata(&path.join("objects"))?.is_some_and(|metadata| metadata.is_dir())
            && metadata(&path.join("refs"))?.is_some_and(|metadata| metadata.is_dir())))
}

fn metadata(path: &Path) -> Result<Option<fs::Metadata>, CiError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CiError::new(format!(
            "cannot inspect {}: {e}",
            path.display()
        ))),
    }
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

pub(super) fn resolve(path: &Path) -> Result<PathBuf, CiError> {
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
