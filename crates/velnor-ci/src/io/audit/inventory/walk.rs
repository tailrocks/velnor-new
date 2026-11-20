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
    overflowed: bool,
}

impl Budget {
    pub(in crate::io::audit) fn new(limit: usize) -> Self {
        Self {
            remaining: limit,
            limit,
            overflowed: false,
        }
    }

    pub(in crate::io::audit) fn overflowed(&self) -> bool {
        self.overflowed
    }

    fn reserve(&mut self) -> io::Result<()> {
        let Some(remaining) = self.remaining.checked_sub(1) else {
            self.overflowed = true;
            return Err(io::Error::other(format!(
                "audit freshness scan limit exceeded: more than {} entries; raise it with {MAX_ENTRIES_VARIABLE} (at most {MAX_ENTRIES_CEILING})",
                self.limit
            )));
        };
        self.remaining = remaining;
        Ok(())
    }
}

pub(in crate::io::audit) fn scan(
    root: &Path,
    targets: &BTreeSet<PathBuf>,
    budget: &mut Budget,
) -> io::Result<Entries> {
    scan_with(root, targets, budget, |directory: &Path| {
        fs::read_dir(directory)
    })
}

pub(super) fn scan_with(
    root: &Path,
    targets: &BTreeSet<PathBuf>,
    budget: &mut Budget,
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
        budget.reserve()?;
        let listing = match open(&directory) {
            Ok(listing) => listing,
            Err(error) if error.kind() == io::ErrorKind::NotFound && directory != root => {
                vanished.push(directory);
                continue;
            }
            Err(error) => return Err(error),
        };
        let mut children = BTreeMap::new();
        for entry in listing {
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
