// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::super::options::{MAX_ENTRIES_CEILING, MAX_ENTRIES_VARIABLE};
use super::{Entries, Kind, excluded_entry};

/// The entries a scan may still walk. One audit shares a budget across its scans, so
/// the limit bounds their total.
pub(in crate::io::audit) struct Budget {
    remaining: usize,
    limit: usize,
}

impl Budget {
    pub(in crate::io::audit) fn new(limit: usize) -> Self {
        Self {
            remaining: limit,
            limit,
        }
    }

    fn reserve(&mut self) -> io::Result<()> {
        self.remaining = self.remaining.checked_sub(1).ok_or_else(|| {
            io::Error::other(format!(
                "audit freshness scan limit exceeded: more than {} entries; raise it with {MAX_ENTRIES_VARIABLE} (at most {MAX_ENTRIES_CEILING})",
                self.limit
            ))
        })?;
        Ok(())
    }
}

pub(in crate::io::audit) fn scan(
    root: &Path,
    targets: &BTreeSet<PathBuf>,
    budget: &mut Budget,
) -> io::Result<Entries> {
    let root = fs::canonicalize(root)?;
    let mut entries = Entries::new();
    let mut pending = vec![root.clone()];
    let mut visited = BTreeSet::new();
    while let Some(directory) = pending.pop() {
        if !visited.insert(directory.clone()) {
            continue;
        }
        budget.reserve()?;
        let mut children = BTreeMap::new();
        for entry in fs::read_dir(&directory)? {
            budget.reserve()?;
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

pub(in crate::io::audit) fn unresolved_link(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound || error.raw_os_error() == Some(libc::ELOOP)
}
