// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use super::inputs::tracked_paths;
use crate::error::CiError;

static NEXT_PROBE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaseSensitivity {
    Sensitive,
    Insensitive,
}

pub(super) fn check_target(root: &Path, target: &Path) -> Result<(), CiError> {
    let target = root.join(target);
    check_input_target_with(root, &target, &|path| {
        probe_case_sensitivity(path, &[&target, root])
    })
}

pub fn check_input_target_with(
    root: &Path,
    target: &Path,
    probe: &impl Fn(&Path) -> Result<CaseSensitivity, CiError>,
) -> Result<(), CiError> {
    let path = root.join(target);
    let object = target_object(&path)?;
    let mut targets = vec![object, resolve(&path)?];
    targets.dedup();
    let links = symlink_objects(&path)?;
    if targets.iter().chain(links.iter()).any(|target| {
        target.components().any(|part| {
            part.as_os_str()
                .as_encoded_bytes()
                .eq_ignore_ascii_case(b".cargo")
        })
    }) {
        return Err(CiError::new(format!(
            "target directory {} overlaps Cargo configuration",
            path.display()
        )));
    }
    let mut sensitivity = CaseSensitivity::Sensitive;
    for path in targets.iter().chain(links.iter()) {
        if probe(path)? == CaseSensitivity::Insensitive {
            sensitivity = CaseSensitivity::Insensitive;
        }
    }
    let top = protect_checkout(root, &targets, &links, sensitivity)?;
    let mut checked = BTreeSet::from([top]);
    for target in targets.iter().chain(links.iter()) {
        for ancestor in target.ancestors() {
            if !checked.contains(ancestor) && git_marker(ancestor)? {
                protect_checkout(ancestor, &targets, &links, sensitivity)?;
                checked.insert(ancestor.to_path_buf());
            }
        }
    }
    Ok(())
}

pub(super) fn check_removal_target(target: &Path) -> Result<(), CiError> {
    let object = target_object(target)?;
    let resolved = resolve(target)?;
    protect_tree(&object)?;
    if resolved != object {
        protect_tree(&resolved)?;
    }
    Ok(())
}

pub(super) fn target_object(path: &Path) -> Result<PathBuf, CiError> {
    match path.parent().zip(path.file_name()) {
        Some((parent, name)) => Ok(resolve(parent)?.join(name)),
        None => resolve(path),
    }
}

fn symlink_objects(path: &Path) -> Result<BTreeSet<PathBuf>, CiError> {
    let mut pending = vec![path.to_path_buf()];
    let mut links = BTreeSet::new();
    while let Some(path) = pending.pop() {
        for ancestor in path.ancestors() {
            if !metadata(ancestor)?.is_some_and(|metadata| metadata.is_symlink()) {
                continue;
            }
            if links.insert(target_object(ancestor)?) {
                let destination = fs::read_link(ancestor).map_err(|e| {
                    CiError::new(format!("cannot read symlink {}: {e}", ancestor.display()))
                })?;
                pending.push(ancestor.with_file_name(destination));
            }
        }
    }
    Ok(links)
}

fn protect_checkout(
    root: &Path,
    targets: &[PathBuf],
    links: &BTreeSet<PathBuf>,
    sensitivity: CaseSensitivity,
) -> Result<PathBuf, CiError> {
    let root = resolve(root)?;
    let tracked = tracked_paths(&root)?;
    let top = resolve(&tracked.top)?;
    let metadata = [
        resolve(&top.join(".git"))?,
        git_path(&root, "--absolute-git-dir")?,
        git_path(&root, "--git-common-dir")?,
    ];
    if targets.iter().any(|target| {
        starts_with(&root, target, sensitivity)
            || metadata.iter().any(|path| {
                starts_with(path, target, sensitivity) || starts_with(target, path, sensitivity)
            })
            || tracked.paths.iter().map(|path| top.join(path)).any(|path| {
                starts_with(&path, target, sensitivity)
                    || links
                        .iter()
                        .any(|link| starts_with(&path, link, sensitivity))
            })
    }) {
        return Err(CiError::new(format!(
            "target directory {} contains checkout files",
            targets[0].display()
        )));
    }
    Ok(top)
}

fn starts_with(path: &Path, prefix: &Path, sensitivity: CaseSensitivity) -> bool {
    if sensitivity == CaseSensitivity::Sensitive {
        return path.starts_with(prefix);
    }
    let mut components = path.components();
    let mut unicode = false;
    let overlaps = prefix.components().all(|prefix| {
        components.next().is_some_and(|component| {
            let left = component.as_os_str().as_encoded_bytes();
            let right = prefix.as_os_str().as_encoded_bytes();
            if left.eq_ignore_ascii_case(right) {
                return true;
            }
            let uncertain = !left.is_ascii() || !right.is_ascii();
            unicode |= uncertain;
            uncertain
        })
    });
    if !overlaps || !unicode {
        return overlaps;
    }
    let depth = path.components().count() - prefix.components().count();
    let Some(ancestor) = path.ancestors().nth(depth) else {
        return true;
    };
    match (fs::canonicalize(ancestor), fs::canonicalize(prefix)) {
        (Ok(path), Ok(prefix)) => path == prefix,
        _ => true,
    }
}

fn nearest_directory(path: &Path) -> Result<&Path, CiError> {
    let mut parent = None;
    for ancestor in path.ancestors() {
        match fs::metadata(ancestor) {
            Ok(metadata) if metadata.is_dir() => {
                parent = Some(ancestor);
                break;
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(CiError::new(format!(
                    "cannot inspect {}: {error}",
                    ancestor.display()
                )));
            }
        }
    }
    parent.ok_or_else(|| CiError::new(format!("cannot find filesystem for {}", path.display())))
}

fn probe_case_sensitivity(path: &Path, candidates: &[&Path]) -> Result<CaseSensitivity, CiError> {
    let path = if metadata(path)?.is_some_and(|metadata| metadata.is_symlink()) {
        path.parent()
            .ok_or_else(|| CiError::new("symlink has no parent"))?
    } else {
        path
    };
    let parent = nearest_directory(path)?;
    let mut parents = Vec::new();
    for candidate in candidates.iter().copied().chain([parent]) {
        let directory = nearest_directory(candidate)?;
        if same_filesystem(directory, parent)? && !parents.contains(&directory) {
            parents.push(directory);
        }
    }
    let mut failure = None;
    let mut created = None;
    for parent in parents {
        loop {
            let sequence = NEXT_PROBE.fetch_add(1, Ordering::Relaxed);
            let directory = parent.join(format!(
                ".velnor-case-probe-{}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&directory) {
                Ok(()) => {
                    created = Some(directory);
                    break;
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        if created.is_some() {
            break;
        }
    }
    let directory = created.ok_or_else(|| {
        CiError::new(format!(
            "cannot probe filesystem at {}: {}",
            parent.display(),
            failure.map_or_else(
                || "no writable directory on the same filesystem".to_owned(),
                |error| error.to_string()
            )
        ))
    })?;
    let file = directory.join("probe");
    let result = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&file)
        .and_then(|_| match fs::metadata(directory.join("PROBE")) {
            Ok(_) => Ok(CaseSensitivity::Insensitive),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(CaseSensitivity::Sensitive),
            Err(error) => Err(error),
        });
    let removed_file = match fs::remove_file(&file) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    };
    let removed_directory = fs::remove_dir(&directory);
    result
        .and_then(|result| removed_file.and(removed_directory).map(|()| result))
        .map_err(|error| {
            CiError::new(format!(
                "cannot probe filesystem at {}: {error}",
                parent.display()
            ))
        })
}

#[cfg(unix)]
fn same_filesystem(left: &Path, right: &Path) -> Result<bool, CiError> {
    use std::os::unix::fs::MetadataExt;

    let device = |path: &Path| {
        fs::metadata(path)
            .map(|metadata| metadata.dev())
            .map_err(|error| CiError::new(format!("cannot inspect {}: {error}", path.display())))
    };
    Ok(device(left)? == device(right)?)
}

#[cfg(not(unix))]
fn same_filesystem(left: &Path, right: &Path) -> Result<bool, CiError> {
    Ok(resolve(left)? == resolve(right)?)
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
        Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => Ok(None),
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
