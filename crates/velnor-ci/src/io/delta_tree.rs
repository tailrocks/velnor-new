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
    inventory_with(root, filtered, &mut |_| {})
}

/// Lists the tree under `root`. Concurrent cargo lanes sharing a target directory create and
/// remove directories while this walk runs. A directory that vanishes after its parent listed it
/// is left out, together with its entries: nothing is listed from a directory that is gone when it
/// is read. A vanished directory cannot be a checkout that a removal would delete, and a delta
/// that misses an entry is already possible for a file created after its directory was listed.
/// Every other error still fails the walk. `before_read` runs just before each directory is read.
fn inventory_with(
    root: &Path,
    filtered: bool,
    before_read: &mut dyn FnMut(&Path),
) -> Result<BTreeSet<PathBuf>, CiError> {
    safe_path(root)?;
    let mut result = BTreeSet::new();
    if !root.exists() {
        return Ok(result);
    }
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        let dir = root.join(&relative);
        before_read(&dir);
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                result.remove(&relative);
                continue;
            }
            Err(e) => return Err(failure(&dir, &e)),
        };
        for entry in entries {
            let entry = entry.map_err(|e| failure(&dir, &e))?;
            let path = relative.join(entry.file_name());
            let kind = match entry.file_type() {
                Ok(kind) => kind,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(failure(&entry.path(), &e)),
            };
            if filtered
                && ((kind.is_dir()
                    && entry.file_name() == "incremental"
                    && matches!(path.components().count(), 2 | 3))
                    || (path.starts_with(".velnor")
                        && path != Path::new(".velnor")
                        && !crate::inputs::is_inputs_file(&path)))
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

pub(super) fn validate(
    manifest: &Manifest,
    staging: &Path,
    target: &Path,
    cutoff: u64,
) -> Result<(), CiError> {
    manifest.sources.records()?;
    snapshot::validate_times(&manifest.nodes, cutoff)?;
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
    if manifest
        .nodes
        .keys()
        .any(|path| crate::inputs::is_inputs_file(path))
    {
        expected.insert(PathBuf::from(".velnor"));
    }
    if inventory(&staging.join("files"), false)? != expected {
        return Err(CiError::new(
            "delta payload node set does not match its manifest",
        ));
    }
    Ok(())
}

pub(super) fn apply(
    manifest: &Manifest,
    staging: &Path,
    target: &Path,
    cutoff: u64,
) -> Result<usize, CiError> {
    validate(manifest, staging, target, cutoff)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        let exe = std::env::current_exe().expect("fixture step");
        let root = exe
            .parent()
            .expect("fixture step")
            .join(format!("velnor-delta-tree-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("gone")).expect("fixture step");
        fs::write(root.join("gone").join("inner"), b"x").expect("fixture step");
        fs::write(root.join("keep"), b"x").expect("fixture step");
        root
    }

    #[test]
    fn directory_removed_after_listing_is_left_out() {
        let root = fixture("vanished");
        let result = inventory_with(&root, false, &mut |dir| {
            if dir.ends_with("gone") {
                fs::remove_dir_all(dir).expect("fixture step");
            }
        });
        fs::remove_dir_all(&root).expect("fixture step");
        assert_eq!(
            result.expect("fixture step"),
            BTreeSet::from([PathBuf::from("keep")])
        );
    }

    #[test]
    fn unreadable_directory_still_fails_the_walk() {
        let root = fixture("unreadable");
        let gone = root.join("gone");
        fs::set_permissions(&gone, fs::Permissions::from_mode(0o000)).expect("fixture step");
        let readable = fs::read_dir(&gone).is_ok();
        let result = inventory_with(&root, false, &mut |_| {});
        fs::set_permissions(&gone, fs::Permissions::from_mode(0o700)).expect("fixture step");
        fs::remove_dir_all(&root).expect("fixture step");
        if readable {
            // Running as root: permissions do not stop the read, so there is no error to see.
            return;
        }
        assert!(result.is_err());
    }
}
