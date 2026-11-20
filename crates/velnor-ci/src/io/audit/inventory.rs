// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod evidence;

pub(super) use evidence::{Evidence, State, state};

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use velnor_rust::{normalize, sha256_hex};

use super::super::units::Workspace;

pub(super) const LIMIT: usize = 100_000;
pub(super) const STORE: &str = ".velnor-input-inventories";
pub(super) type Entries = BTreeMap<PathBuf, BTreeMap<PathBuf, Kind>>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Kind {
    File,
    Directory,
    Link {
        target: PathBuf,
        resolved: Option<PathBuf>,
    },
    Other,
}

#[derive(Serialize, Deserialize)]
pub(super) struct Receipt {
    pub(super) output: String,
    pub(super) modified: SystemTime,
    pub(super) inventory: PathBuf,
    pub(super) digest: String,
}

pub(super) fn exclusions(workspaces: &[Workspace]) -> BTreeSet<PathBuf> {
    workspaces
        .iter()
        .flat_map(|workspace| {
            [
                workspace.workspace_root.join("target"),
                workspace.target_directory.clone(),
            ]
        })
        .map(|target| velnor_rust::resolve(&target))
        .collect()
}

pub(super) fn excluded(path: &Path, root: &Path, targets: &BTreeSet<PathBuf>) -> bool {
    targets.iter().any(|target| {
        if root.starts_with(target) {
            target == root
                && path.strip_prefix(target).is_ok_and(|relative| {
                    relative.components().next().is_some_and(|name| {
                        matches!(
                            name.as_os_str().to_str(),
                            Some("debug" | "release" | STORE | ".rustc_info.json" | "CACHEDIR.TAG")
                        )
                    })
                })
        } else {
            path.starts_with(target)
        }
    })
}

fn excluded_entry(path: &Path, kind: &Kind, root: &Path, targets: &BTreeSet<PathBuf>) -> bool {
    excluded(path, root, targets)
        || match kind {
            Kind::Link {
                resolved: Some(resolved),
                ..
            } => excluded(resolved, root, targets),
            Kind::Link {
                target,
                resolved: None,
            } => {
                let lexical = normalize(&path.parent().unwrap_or(path).join(target));
                excluded(&lexical, root, targets)
            }
            Kind::File | Kind::Directory | Kind::Other => false,
        }
}

pub(super) fn scan(
    root: &Path,
    targets: &BTreeSet<PathBuf>,
    remaining: &mut usize,
) -> io::Result<Entries> {
    let root = fs::canonicalize(root)?;
    let mut entries = Entries::new();
    let mut pending = vec![root.clone()];
    let mut visited = BTreeSet::new();
    while let Some(directory) = pending.pop() {
        if !visited.insert(directory.clone()) {
            continue;
        }
        reserve(remaining)?;
        let mut children = BTreeMap::new();
        for entry in fs::read_dir(&directory)? {
            reserve(remaining)?;
            let entry = entry?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            let kind = if metadata.is_symlink() {
                let resolved = match fs::canonicalize(&path) {
                    Ok(resolved) => Some(resolved),
                    Err(error) if unresolved_link(&error) => None,
                    Err(error) => return Err(error),
                };
                Kind::Link {
                    target: fs::read_link(&path)?,
                    resolved,
                }
            } else if metadata.is_dir() {
                Kind::Directory
            } else if metadata.is_file() {
                Kind::File
            } else {
                Kind::Other
            };
            if !excluded_entry(&path, &kind, &root, targets) {
                match &kind {
                    Kind::Directory => pending.push(path),
                    Kind::Link {
                        resolved: Some(resolved),
                        ..
                    } if fs::metadata(resolved)?.is_dir() => pending.push(resolved.clone()),
                    Kind::File | Kind::Link { .. } | Kind::Other => {}
                }
            }
            children.insert(PathBuf::from(entry.file_name()), kind);
        }
        entries.insert(directory, children);
    }
    Ok(entries)
}

fn reserve(remaining: &mut usize) -> io::Result<()> {
    *remaining = remaining
        .checked_sub(1)
        .ok_or_else(|| io::Error::other("audit freshness scan limit exceeded"))?;
    Ok(())
}

pub(super) fn unresolved_link(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound || error.raw_os_error() == Some(libc::ELOOP)
}

pub(super) fn view(
    entries: &Entries,
    root: &Path,
    targets: &BTreeSet<PathBuf>,
) -> io::Result<Entries> {
    let mut found = Entries::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        if found.contains_key(&directory) {
            continue;
        }
        let children = entries
            .get(&directory)
            .ok_or_else(|| io::Error::other("declared directory has no pre-build inventory"))?;
        let mut included = BTreeMap::new();
        for (name, kind) in children {
            let path = directory.join(name);
            if excluded_entry(&path, kind, root, targets) {
                continue;
            }
            match kind {
                Kind::Directory => pending.push(path),
                Kind::Link {
                    resolved: Some(resolved),
                    ..
                } if entries.contains_key(resolved) => pending.push(resolved.clone()),
                Kind::File | Kind::Link { .. } | Kind::Other => {}
            }
            included.insert(name.clone(), kind.clone());
        }
        found.insert(directory, included);
    }
    Ok(found)
}

fn resolve(entries: &Entries, path: &Path) -> Option<PathBuf> {
    let mut root = path
        .ancestors()
        .find(|ancestor| entries.contains_key(*ancestor))?
        .to_path_buf();
    let relative = path.strip_prefix(&root).ok()?.to_path_buf();
    for component in relative.components() {
        match component {
            Component::Normal(name) => {
                let next = root.join(name);
                if entries.contains_key(&next) {
                    root = next;
                    continue;
                }
                let kind = entries.get(&root)?.get(Path::new(name))?;
                root = match kind {
                    Kind::Directory => root.join(name),
                    Kind::Link {
                        resolved: Some(resolved),
                        ..
                    } => resolved.clone(),
                    Kind::File | Kind::Link { .. } | Kind::Other => return None,
                };
            }
            Component::ParentDir if known_directory(entries, &root) => {
                root = root.parent()?.to_path_buf();
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    entries.contains_key(&root).then_some(root)
}

fn known_directory(entries: &Entries, directory: &Path) -> bool {
    entries
        .range(directory.to_path_buf()..)
        .next()
        .is_some_and(|(node, _)| node.starts_with(directory))
        || directory
            .parent()
            .zip(directory.file_name())
            .is_some_and(|(parent, name)| {
                entries
                    .get(parent)
                    .and_then(|children| children.get(Path::new(name)))
                    == Some(&Kind::Directory)
            })
}

pub(super) fn receipt_path(output: &Path) -> PathBuf {
    output.with_extension("velnor-inputs.json")
}

pub(super) fn receipt(output: &Path, inventory: PathBuf, digest: String) -> io::Result<Receipt> {
    Ok(Receipt {
        output: sha256_hex(&fs::read(output)?),
        modified: fs::metadata(output)?.modified()?,
        inventory,
        digest,
    })
}

pub(super) fn matches(
    output: &Path,
    declared: &Path,
    current: &Entries,
    targets: &BTreeSet<PathBuf>,
) -> io::Result<bool> {
    let saved = load(output)?;
    let Some(root) = resolve(&saved.entries, declared) else {
        return Ok(false);
    };
    if root != fs::canonicalize(declared)? || saved.targets != *targets {
        return Ok(false);
    }
    Ok(view(&saved.entries, &root, targets)? == *current)
}

pub(super) fn load(output: &Path) -> io::Result<Evidence> {
    let receipt: Receipt = serde_json::from_slice(&fs::read(receipt_path(output))?)?;
    if receipt.output != sha256_hex(&fs::read(output)?)
        || receipt.modified != fs::metadata(output)?.modified()?
    {
        return Err(io::Error::other(
            "build output no longer matches its inventory",
        ));
    }
    let bytes = fs::read(receipt.inventory)?;
    if sha256_hex(&bytes) != receipt.digest {
        return Err(io::Error::other("directory inventory digest differs"));
    }
    Ok(serde_json::from_slice(&bytes)?)
}
