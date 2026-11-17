// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use velnor_rust::{normalize, sha256_hex};

use super::super::units::Workspace;

pub(super) const LIMIT: usize = 100_000;
pub(super) const STORE: &str = ".velnor-input-inventories";
pub(super) type Entries = BTreeMap<PathBuf, Kind>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Kind {
    File,
    Directory,
    Link(PathBuf),
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
        .map(|target| fs::canonicalize(&target).unwrap_or_else(|_| normalize(&target)))
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

pub(super) fn scan(
    root: &Path,
    targets: &BTreeSet<PathBuf>,
    remaining: &mut usize,
) -> io::Result<Entries> {
    let root = fs::canonicalize(root)?;
    let mut entries = Entries::new();
    let mut pending = vec![root.clone()];
    let mut visited = BTreeSet::new();
    while let Some(path) = pending.pop() {
        if excluded(&path, &root, targets)
            || fs::canonicalize(&path).is_ok_and(|canonical| excluded(&canonical, &root, targets))
        {
            continue;
        }
        *remaining = remaining
            .checked_sub(1)
            .ok_or_else(|| io::Error::other("audit freshness scan limit exceeded"))?;
        let metadata = fs::symlink_metadata(&path)?;
        let kind = if metadata.is_symlink() {
            Kind::Link(fs::read_link(&path)?)
        } else if metadata.is_dir() {
            Kind::Directory
        } else if metadata.is_file() {
            Kind::File
        } else {
            Kind::Other
        };
        entries.insert(path.clone(), kind);
        if fs::metadata(&path)?.is_dir() && visited.insert(fs::canonicalize(&path)?) {
            for entry in fs::read_dir(path)? {
                pending.push(entry?.path());
            }
        }
    }
    Ok(entries)
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

pub(super) fn matches(output: &Path, directory: &Path, current: &Entries) -> io::Result<bool> {
    let saved = load(output)?;
    let saved = saved
        .into_iter()
        .filter(|(path, _)| path.starts_with(directory))
        .collect::<Entries>();
    Ok(saved == *current)
}

pub(super) fn contained_directory(output: &Path, directory: &Path) -> bool {
    load(output).is_ok_and(|saved| saved.get(&normalize(directory)) == Some(&Kind::Directory))
}

fn load(output: &Path) -> io::Result<Entries> {
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
