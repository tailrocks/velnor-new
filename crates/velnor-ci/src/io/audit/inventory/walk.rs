// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::super::options::{MAX_ENTRIES_CEILING, MAX_ENTRIES_VARIABLE};
use super::handle::{self, Opened, Origin};
use super::{Entries, Kind, excluded_entry};

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
    scan_with(root, targets, budget, &Probe::default())
}

pub(super) struct Probe<'a> {
    pub(super) before_open: &'a dyn Fn(&Path) -> io::Result<()>,
    pub(super) after_open: &'a dyn Fn(&Path),
    pub(super) listed: &'a dyn Fn(&OsStr),
}

impl Default for Probe<'_> {
    fn default() -> Self {
        Self {
            before_open: &|_| Ok(()),
            after_open: &|_| {},
            listed: &|_| {},
        }
    }
}

struct Pending {
    path: PathBuf,
    origin: Origin,
}

pub(super) fn scan_with(
    root: &Path,
    targets: &BTreeSet<PathBuf>,
    budget: &mut Budget,
    probe: &Probe<'_>,
) -> io::Result<Entries> {
    let root = fs::canonicalize(root)?;
    let mut entries = Entries::new();
    let mut pending = vec![Pending {
        path: root.clone(),
        origin: Origin::Path,
    }];
    let mut visited = BTreeSet::new();
    while let Some(Pending {
        path: directory,
        origin,
    }) = pending.pop()
    {
        if !visited.insert(directory.clone()) {
            continue;
        }
        budget.reserve()?;
        let opened =
            (probe.before_open)(&directory).and_then(|()| handle::open(&origin, &directory));
        let mut listing = match opened {
            Ok(listing) => listing,
            Err(error) if handle::replaced(&error) && directory != root => {
                if let Origin::Child { parent, name } = &origin {
                    let replacement = handle::kind_at(parent, name, &directory)?;
                    settle(
                        &mut entries,
                        &mut pending,
                        &directory,
                        parent,
                        name,
                        replacement,
                        (&root, targets),
                    )?;
                }
                continue;
            }
            Err(error) => return Err(error),
        };
        (probe.after_open)(&directory);
        let names = handle::names(&mut listing, |name| {
            (probe.listed)(name);
            budget.reserve()
        })?;
        let listing = Opened::new(listing);
        let mut children = BTreeMap::new();
        for name in names {
            let path = directory.join(&name);
            let Some(kind) = handle::kind_at(&listing, &name, &path)? else {
                continue;
            };
            pending.extend(queue(&kind, &path, &listing, &name, (&root, targets))?);
            children.insert(PathBuf::from(name), kind);
        }
        entries.insert(directory, children);
    }
    Ok(entries)
}

/// What to walk next for a classified child: a directory through its parent's
/// handle, or the directory a link resolves to.
fn queue(
    kind: &Kind,
    path: &Path,
    parent: &Opened,
    name: &OsStr,
    (root, targets): (&Path, &BTreeSet<PathBuf>),
) -> io::Result<Option<Pending>> {
    if excluded_entry(path, kind, root, targets) {
        return Ok(None);
    }
    Ok(match kind {
        Kind::Directory => Some(Pending {
            path: path.to_path_buf(),
            origin: Origin::Child {
                parent: Opened::clone(parent),
                name: name.to_os_string(),
            },
        }),
        Kind::Link {
            resolved: Some(resolved),
            ..
        } if is_directory(resolved)? => Some(Pending {
            path: resolved.clone(),
            origin: Origin::Path,
        }),
        Kind::File | Kind::Link { .. } | Kind::Other => None,
    })
}

/// A queued directory could not be opened because its entry changed after it was
/// classified. A vanished entry is dropped from its parent. Anything else that now
/// sits there (a link, a file) is inventoried in its place, and a directory that
/// reappeared fails the scan, because it was not walked.
fn settle(
    entries: &mut Entries,
    pending: &mut Vec<Pending>,
    directory: &Path,
    parent: &Opened,
    name: &OsStr,
    replacement: Option<Kind>,
    (root, targets): (&Path, &BTreeSet<PathBuf>),
) -> io::Result<()> {
    let children = directory
        .parent()
        .and_then(|container| entries.get_mut(container));
    let key = Path::new(name);
    match replacement {
        None => {
            if let Some(children) = children {
                children.remove(key);
            }
        }
        Some(Kind::Directory) => {
            return Err(io::Error::other(format!(
                "{} changed while the audit scanned it; retry",
                directory.display()
            )));
        }
        Some(kind) => {
            pending.extend(queue(&kind, directory, parent, name, (root, targets))?);
            if let Some(children) = children {
                children.insert(key.to_path_buf(), kind);
            }
        }
    }
    Ok(())
}

fn is_directory(path: &Path) -> io::Result<bool> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_dir()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

pub(in crate::io::audit) fn unresolved_link(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound
        || matches!(error.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR))
}
