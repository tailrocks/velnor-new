// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
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
}

impl Default for Probe<'_> {
    fn default() -> Self {
        Self {
            before_open: &|_| Ok(()),
            after_open: &|_| {},
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
    let mut vanished = Vec::new();
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
                vanished.push(directory);
                continue;
            }
            Err(error) => return Err(error),
        };
        (probe.after_open)(&directory);
        let names = handle::names(&mut listing)?;
        let listing = Opened::new(listing);
        let mut children = BTreeMap::new();
        for name in names {
            budget.reserve()?;
            let path = directory.join(&name);
            let Some(kind) = handle::kind_at(&listing, &name, &path)? else {
                continue;
            };
            if !excluded_entry(&path, &kind, &root, targets) {
                match &kind {
                    Kind::Directory => pending.push(Pending {
                        path,
                        origin: Origin::Child {
                            parent: Opened::clone(&listing),
                            name: name.clone(),
                        },
                    }),
                    Kind::Link {
                        resolved: Some(resolved),
                        ..
                    } if is_directory(resolved)? => pending.push(Pending {
                        path: resolved.clone(),
                        origin: Origin::Path,
                    }),
                    Kind::File | Kind::Link { .. } | Kind::Other => {}
                }
            }
            children.insert(PathBuf::from(name), kind);
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
