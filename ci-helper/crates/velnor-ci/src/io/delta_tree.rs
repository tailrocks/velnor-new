// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::delta::{failure, safe_path};
use super::delta_snapshot::{self as snapshot, Manifest, Node};
use super::inputs::set_modified_ns;
use crate::error::CiError;

pub(super) fn paths(root: &Path) -> Result<BTreeSet<PathBuf>, CiError> {
    inventory(root, true)
}

fn inventory(root: &Path, filtered: bool) -> Result<BTreeSet<PathBuf>, CiError> {
    safe_path(root)?;
    let mut result = BTreeSet::new();
    if !root.exists() {
        return Ok(result);
    }
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        let dir = root.join(&relative);
        for entry in fs::read_dir(&dir).map_err(|e| failure(&dir, &e))? {
            let entry = entry.map_err(|e| failure(&dir, &e))?;
            let path = relative.join(entry.file_name());
            let kind = entry.file_type().map_err(|e| failure(&entry.path(), &e))?;
            if filtered
                && ((kind.is_dir()
                    && entry.file_name() == "incremental"
                    && matches!(path.components().count(), 2 | 3))
                    || (path.starts_with(".velnor")
                        && path != Path::new(".velnor")
                        && path != Path::new(".velnor/inputs.tsv")))
            {
                continue;
            }
            if !kind.is_dir() && !kind.is_file() {
                return Err(CiError::new(format!(
                    "{} is not a regular delta file or directory",
                    entry.path().display()
                )));
            }
            if kind.is_dir() {
                pending.push(path.clone());
            }
            if !filtered || path != Path::new(".velnor") {
                result.insert(path);
            }
        }
    }
    Ok(result)
}

pub(super) fn remove_node(path: &Path) -> Result<(), CiError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .map_err(|e| failure(path, &e))?;
            for entry in fs::read_dir(path).map_err(|e| failure(path, &e))? {
                let entry = entry.map_err(|e| failure(path, &e))?;
                remove_node(&entry.path())?;
            }
            fs::remove_dir(path).map_err(|e| failure(path, &e))
        }
        Ok(_) => fs::remove_file(path).map_err(|e| failure(path, &e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(failure(path, &e)),
    }
}

pub(super) fn metadata(path: &Path, node: &Node) -> Result<(), CiError> {
    let (modified, mode) = node.metadata();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|e| failure(path, &e))?;
    set_modified_ns(path, modified)
}

pub(super) fn copy(from: &Path, to: &Path, node: &Node) -> Result<(), CiError> {
    safe_path(from)?;
    safe_path(to)?;
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|e| failure(parent, &e))?;
    }
    remove_node(to)?;
    fs::copy(from, to).map_err(|e| failure(to, &e))?;
    metadata(to, node)
}

pub(super) fn write_nodes(
    from: &Path,
    to: &Path,
    nodes: &BTreeMap<PathBuf, Node>,
) -> Result<(), CiError> {
    let mut directories: Vec<_> = nodes.iter().filter(|(_, node)| node.directory()).collect();
    directories.sort_by_key(|(path, _)| path.components().count());
    for (path, _) in &directories {
        let destination = to.join(path);
        fs::create_dir_all(&destination).map_err(|e| failure(&destination, &e))?;
        fs::set_permissions(&destination, fs::Permissions::from_mode(0o700))
            .map_err(|e| failure(&destination, &e))?;
    }
    for (path, node) in nodes.iter().filter(|(_, node)| !node.directory()) {
        copy(&from.join(path), &to.join(path), node)?;
    }
    for (path, node) in directories.into_iter().rev() {
        metadata(&to.join(path), node)?;
    }
    Ok(())
}

fn validate(manifest: &Manifest, staging: &Path, target: &Path) -> Result<(), CiError> {
    let size = manifest
        .nodes
        .values()
        .fold(0_u64, |size, node| size.saturating_add(node.size()));
    if size.saturating_add(snapshot::encode(manifest)?.len() as u64) > super::delta::LIMIT {
        return Err(CiError::new("delta exceeds its byte limit"));
    }
    let payload = staging.join("files");
    safe_path(&payload)?;
    if !fs::symlink_metadata(&payload).is_ok_and(|metadata| metadata.is_dir()) {
        return Err(CiError::new("delta payload must be a directory"));
    }
    for path in manifest.nodes.keys().chain(&manifest.removed) {
        snapshot::validate(path)?;
        safe_path(&target.join(path))?;
    }
    for (path, node) in &manifest.nodes {
        for parent in path
            .ancestors()
            .skip(1)
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            if parent == Path::new(".velnor") {
                continue;
            }
            if !manifest.nodes.get(parent).is_some_and(Node::directory) {
                return Err(CiError::new("delta manifest omits an ancestor directory"));
            }
        }
        let source = staging.join("files").join(path);
        let stat = snapshot::stat(&source)?;
        if stat.directory() != node.directory()
            || stat.size() != node.size()
            || stat.metadata() != node.metadata()
            || snapshot::identity(&source)? != *node
        {
            return Err(CiError::new("delta payload does not match its manifest"));
        }
    }
    let mut expected: BTreeSet<_> = manifest.nodes.keys().cloned().collect();
    if manifest.nodes.contains_key(Path::new(".velnor/inputs.tsv")) {
        expected.insert(PathBuf::from(".velnor"));
    }
    if inventory(&staging.join("files"), false)? != expected {
        return Err(CiError::new(
            "delta payload node set does not match its manifest",
        ));
    }
    Ok(())
}

pub(super) fn apply(manifest: &Manifest, staging: &Path, target: &Path) -> Result<usize, CiError> {
    validate(manifest, staging, target)?;
    let mut directories: Vec<_> = manifest
        .nodes
        .iter()
        .filter(|(_, node)| node.directory())
        .collect();
    directories.sort_by_key(|(path, _)| path.components().count());
    for (path, _) in directories {
        let destination = target.join(path);
        if fs::symlink_metadata(&destination).is_ok_and(|metadata| metadata.is_dir()) {
            fs::set_permissions(&destination, fs::Permissions::from_mode(0o700))
                .map_err(|e| failure(&destination, &e))?;
        }
    }
    let mut removed = manifest.removed.clone();
    removed.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for path in removed {
        remove_node(&target.join(path))?;
    }
    write_nodes(&staging.join("files"), target, &manifest.nodes)?;
    Ok(manifest
        .nodes
        .values()
        .filter(|node| !node.directory())
        .count())
}
