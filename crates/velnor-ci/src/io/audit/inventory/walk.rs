// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{Entries, Kind, excluded_entry};

pub(in crate::io::audit) fn scan(
    root: &Path,
    targets: &BTreeSet<PathBuf>,
    remaining: &mut usize,
) -> io::Result<Entries> {
    scan_with(root, targets, remaining, |directory: &Path| {
        fs::read_dir(directory)
    })
}

// `open` lists one directory. It is a parameter so tests can make a listing
// fail deterministically.
pub(super) fn scan_with(
    root: &Path,
    targets: &BTreeSet<PathBuf>,
    remaining: &mut usize,
    open: impl Fn(&Path) -> io::Result<fs::ReadDir>,
) -> io::Result<Entries> {
    let root = fs::canonicalize(root)?;
    let mut entries = Entries::new();
    let mut pending = vec![root.clone()];
    let mut visited = BTreeSet::new();
    let mut vanished = Vec::new();
    while let Some(directory) = pending.pop() {
        if !visited.insert(directory.clone()) {
            continue;
        }
        reserve(remaining)?;
        let listing = match open(&directory) {
            Ok(listing) => listing,
            // Listed as a directory, then removed before it was opened. Like a
            // vanished entry (see `kind`), it is dropped from its parent below.
            Err(error) if error.kind() == io::ErrorKind::NotFound && directory != root => {
                vanished.push(directory);
                continue;
            }
            Err(error) => return Err(error),
        };
        let mut children = BTreeMap::new();
        for entry in listing {
            reserve(remaining)?;
            let entry = entry?;
            let path = entry.path();
            let Some(kind) = kind(&path)? else {
                continue;
            };
            if !excluded_entry(&path, &kind, &root, targets) {
                match &kind {
                    Kind::Directory => pending.push(path),
                    Kind::Link {
                        resolved: Some(resolved),
                        ..
                    } if is_directory(resolved)? => pending.push(resolved.clone()),
                    Kind::File | Kind::Link { .. } | Kind::Other => {}
                }
            }
            children.insert(PathBuf::from(entry.file_name()), kind);
        }
        entries.insert(directory, children);
    }
    for directory in vanished {
        let (Some(parent), Some(name)) = (directory.parent(), directory.file_name()) else {
            continue;
        };
        if let Some(children) = entries.get_mut(parent)
            && children.get(Path::new(name)) == Some(&Kind::Directory)
        {
            children.remove(Path::new(name));
        }
    }
    Ok(entries)
}

// `None` means the entry was listed but removed before it could be classified.
//
// Skipping it cannot hide a real input. The walk records what the tree holds
// when it looks, and the audit compares that record with the saved one. An
// entry the saved record has and the tree no longer has is a missing child, so
// the audit goes stale. An entry the saved record never had is not covered by
// it, and a walk that ran a moment earlier or later would not have listed it
// either. Entries that stay in the tree are never skipped: their stat succeeds.
fn kind(path: &Path) -> io::Result<Option<Kind>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !metadata.is_symlink() {
        return Ok(Some(if metadata.is_dir() {
            Kind::Directory
        } else if metadata.is_file() {
            Kind::File
        } else {
            Kind::Other
        }));
    }
    let resolved = match fs::canonicalize(path) {
        Ok(resolved) => Some(resolved),
        Err(error) if unresolved_link(&error) => None,
        Err(error) => return Err(error),
    };
    match fs::read_link(path) {
        Ok(target) => Ok(Some(Kind::Link { target, resolved })),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

// A link target that vanished after it was resolved is not a directory to walk.
fn is_directory(path: &Path) -> io::Result<bool> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_dir()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn reserve(remaining: &mut usize) -> io::Result<()> {
    *remaining = remaining
        .checked_sub(1)
        .ok_or_else(|| io::Error::other("audit freshness scan limit exceeded"))?;
    Ok(())
}

pub(in crate::io::audit) fn unresolved_link(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound || error.raw_os_error() == Some(libc::ELOOP)
}
