// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::super::options::{MAX_ENTRIES_CEILING, MAX_ENTRIES_VARIABLE};
use super::handle::{self, Climb, Current, Opened, Origin, Stamp};
use super::held::Held;
use super::probe::Probe;
use super::resolve::Reach;
use super::{Entries, Kind, Scan, excluded_entry};

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
            return Err(io::Error::other(self.exceeded()));
        };
        self.remaining = remaining;
        Ok(())
    }

    fn exceeded(&self) -> String {
        if self.limit >= MAX_ENTRIES_CEILING {
            format!(
                "audit freshness scan limit exceeded: more than {} entries; the tree must be reduced or split",
                self.limit
            )
        } else {
            format!(
                "audit freshness scan limit exceeded: more than {} entries; raise it with {MAX_ENTRIES_VARIABLE} (at most {MAX_ENTRIES_CEILING})",
                self.limit
            )
        }
    }
}

pub(in crate::io::audit) fn scan(
    root: &Path,
    targets: &BTreeSet<PathBuf>,
    budget: &mut Budget,
) -> io::Result<Scan> {
    scan_with(root, targets, budget, &Probe::default())
}

pub(super) const DESCRIPTOR_BUDGET: usize = 64;
pub(super) const HELD_HANDLES: usize = 48;
pub(super) const UNTRACKED_LISTING: usize = 1;

pub(super) struct Pending {
    pub(super) path: PathBuf,
    pub(super) origin: Origin,
}

pub(super) fn scan_with(
    root: &Path,
    targets: &BTreeSet<PathBuf>,
    budget: &mut Budget,
    probe: &Probe<'_>,
) -> io::Result<Scan> {
    let root = fs::canonicalize(root)?;
    let mut entries = Entries::new();
    let mut identities = BTreeMap::new();
    let mut pending = vec![Pending {
        path: root.clone(),
        origin: Origin::Path,
    }];
    let mut visited = BTreeSet::new();
    let mut held = Held::new(probe);
    while let Some(Pending {
        path: directory,
        origin,
    }) = pending.pop()
    {
        if !visited.insert(directory.clone()) {
            handle::revisited(&identities, &directory, held.reach())?;
            continue;
        }
        budget.reserve()?;
        let opened = probe
            .before_open(&directory)
            .and_then(|()| handle::open(&origin, &directory, &held.climb(&origin)));
        let mut listing = match opened {
            Ok(listing) => listing,
            Err(error) if handle::replaced(&error) && directory != root => {
                if matches!(origin, Origin::Path) {
                    return Err(changed(error.kind(), &directory));
                }
                let current =
                    handle::current(&origin, &directory, held.reach(), &held.climb(&origin))?;
                match current {
                    Current::Directory(_) => return Err(changed(error.kind(), &directory)),
                    current => settle(
                        &mut entries,
                        &mut pending,
                        &directory,
                        &origin,
                        current,
                        (&root, targets),
                        held.reach(),
                    )?,
                }
                continue;
            }
            Err(error) => return Err(error),
        };
        let stamp = handle::stamp(&listing)?;
        identities.insert(directory.clone(), stamp);
        probe.after_open(&directory);
        let names = handle::names(&mut listing, |name| {
            probe.listed(name);
            budget.reserve()
        })?;
        let listing = Opened::new(listing);
        let queued_before = pending.len();
        let children = list(
            (&listing, names),
            (&directory, stamp),
            (&mut pending, &mut held),
            (&root, targets),
        )?;
        held.track(&listing, &directory, &mut pending, queued_before);
        let current = handle::current(&origin, &directory, held.reach(), &held.climb(&origin))?;
        match current {
            Current::Directory(now) if stamp.is(now) => {
                entries.insert(directory, children);
            }
            Current::Directory(_) => {
                return Err(io::Error::other(format!(
                    "{} was replaced by another directory while it was listed",
                    directory.display()
                )));
            }
            _ if directory == root => {
                return Err(io::Error::other(format!(
                    "{} was replaced while it was listed",
                    directory.display()
                )));
            }
            _ if matches!(origin, Origin::Path) => {
                return Err(changed(io::ErrorKind::Other, &directory));
            }
            current => {
                pending.truncate(queued_before);
                settle(
                    &mut entries,
                    &mut pending,
                    &directory,
                    &origin,
                    current,
                    (&root, targets),
                    held.reach(),
                )?;
            }
        }
    }
    Ok(Scan {
        entries,
        identities,
    })
}

fn list(
    (listing, names): (&Opened, Vec<OsString>),
    (directory, stamp): (&Path, Stamp),
    (pending, held): (&mut Vec<Pending>, &mut Held<'_, '_>),
    (root, targets): (&Path, &BTreeSet<PathBuf>),
) -> io::Result<BTreeMap<PathBuf, Kind>> {
    let mut children = BTreeMap::new();
    for name in names {
        let path = directory.join(&name);
        let Some(kind) = handle::kind_at(listing.as_ref(), &name, &path, held.reach())? else {
            continue;
        };
        let child = Origin::Child {
            parent: Opened::clone(listing),
            stamp,
            name: name.clone(),
        };
        pending.extend(queue(&kind, &path, child, (root, targets), held.reach())?);
        children.insert(PathBuf::from(name), kind);
    }
    Ok(children)
}

pub(super) fn changed(kind: io::ErrorKind, directory: &Path) -> io::Error {
    io::Error::new(
        kind,
        format!(
            "{} changed while the audit scanned it; retry",
            directory.display()
        ),
    )
}

fn queue(
    kind: &Kind,
    path: &Path,
    origin: Origin,
    (root, targets): (&Path, &BTreeSet<PathBuf>),
    reach: Reach<'_>,
) -> io::Result<Option<Pending>> {
    if excluded_entry(path, kind, root, targets) {
        return Ok(None);
    }
    Ok(match kind {
        Kind::Directory => Some(Pending {
            path: path.to_path_buf(),
            origin,
        }),
        Kind::Link {
            resolved: Some(resolved),
            ..
        } if is_directory(resolved, reach)? => Some(Pending {
            path: resolved.clone(),
            origin: Origin::Path,
        }),
        Kind::File | Kind::Link { .. } | Kind::Other => None,
    })
}

fn settle(
    entries: &mut Entries,
    pending: &mut Vec<Pending>,
    directory: &Path,
    origin: &Origin,
    current: Current,
    (root, targets): (&Path, &BTreeSet<PathBuf>),
    reach: Reach<'_>,
) -> io::Result<()> {
    let children = directory
        .parent()
        .and_then(|container| entries.get_mut(container));
    let key = directory.file_name().map(Path::new);
    match current {
        Current::Gone => {
            if let (Some(children), Some(key)) = (children, key) {
                children.remove(key);
            }
        }
        Current::Directory(_) => return Err(changed(io::ErrorKind::Other, directory)),
        Current::Other(kind) => {
            pending.extend(queue(
                &kind,
                directory,
                origin.clone(),
                (root, targets),
                reach,
            )?);
            if let (Some(children), Some(key)) = (children, key) {
                children.insert(key.to_path_buf(), kind);
            }
        }
    }
    Ok(())
}

fn is_directory(path: &Path, reach: Reach<'_>) -> io::Result<bool> {
    match handle::current(&Origin::Path, path, reach, &Climb::none()) {
        Ok(Current::Directory(_) | Current::Other(Kind::Link { .. })) => Ok(true),
        Ok(Current::Gone | Current::Other(_)) => Ok(false),
        Err(error) if handle::replaced(&error) => Ok(true),
        Err(error) => Err(error),
    }
}

pub(in crate::io::audit) fn unresolved_link(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound
        || matches!(error.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scan_at_the_ceiling_says_the_tree_must_shrink_not_raise_the_limit() {
        let mut budget = Budget::new(MAX_ENTRIES_CEILING);
        budget.remaining = 0;
        let error = budget.reserve().err().map_or_default(|e| e.to_string());
        assert!(error.contains("more than 10000000 entries"), "{error}");
        assert!(error.contains("reduced or split"), "{error}");
        assert!(!error.contains("raise"), "{error}");
    }

    #[test]
    fn a_scan_below_the_ceiling_says_how_to_raise_the_limit() {
        let mut budget = Budget::new(3);
        budget.remaining = 0;
        let error = budget.reserve().err().map_or_default(|e| e.to_string());
        assert!(error.contains(MAX_ENTRIES_VARIABLE), "{error}");
        assert!(error.contains("raise it with"), "{error}");
    }
}
