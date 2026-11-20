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
    let mut replaced = Vec::new();
    while let Some(directory) = pending.pop() {
        if !visited.insert(directory.clone()) {
            continue;
        }
        budget.reserve()?;
        // A queued directory was a real one (never a link) when it was listed. A
        // successful open may have followed a replacement link, so revalidate it too.
        let listing = match open(&directory) {
            Ok(listing) if directory == root => listing,
            Ok(listing) => match kind(&directory)? {
                Some(Kind::Directory) => listing,
                current => {
                    replace(
                        directory,
                        current,
                        &root,
                        targets,
                        &mut pending,
                        &mut replaced,
                    )?;
                    continue;
                }
            },
            Err(error) if moved_before_open(&error) && directory != root => {
                let current = kind(&directory)?;
                if current == Some(Kind::Directory) {
                    return Err(error);
                }
                replace(
                    directory,
                    current,
                    &root,
                    targets,
                    &mut pending,
                    &mut replaced,
                )?;
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
    for (directory, current) in replaced {
        let (Some(parent), Some(name)) = (directory.parent(), directory.file_name()) else {
            continue;
        };
        if let Some(children) = entries.get_mut(parent)
            && children.get(Path::new(name)) == Some(&Kind::Directory)
        {
            match current {
                Some(current) => {
                    children.insert(PathBuf::from(name), current);
                }
                None => {
                    children.remove(Path::new(name));
                }
            }
        }
    }
    Ok(entries)
}

/// Records a queued directory that is no longer one. A link to a directory is walked, as
/// a stable scan would, so the inventory still covers the tree its reads now reach.
fn replace(
    directory: PathBuf,
    current: Option<Kind>,
    root: &Path,
    targets: &BTreeSet<PathBuf>,
    pending: &mut Vec<PathBuf>,
    replaced: &mut Vec<(PathBuf, Option<Kind>)>,
) -> io::Result<()> {
    if let Some(
        link @ Kind::Link {
            resolved: Some(resolved),
            ..
        },
    ) = &current
        && !excluded_entry(&directory, link, root, targets)
        && is_directory(resolved)?
    {
        pending.push(resolved.clone());
    }
    replaced.push((directory, current));
    Ok(())
}

fn moved_before_open(error: &io::Error) -> bool {
    error.raw_os_error() == Some(libc::ELOOP)
        || matches!(
            error.kind(),
            io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
        )
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
