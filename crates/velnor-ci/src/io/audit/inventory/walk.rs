// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use super::super::options::{MAX_ENTRIES_CEILING, MAX_ENTRIES_VARIABLE};
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
    scan_with(root, targets, budget, |directory: &Path| {
        fs::read_dir(directory)
    })
}

type Identity = (u64, u64);

struct Queued {
    path: PathBuf,
    identity: Identity,
}

struct Seen {
    kind: Kind,
    identity: Identity,
}

enum Listed {
    Children(BTreeMap<PathBuf, Kind>),
    Replaced(Option<Kind>),
}

type Replacements = Vec<(PathBuf, Option<Kind>)>;

pub(super) fn scan_with(
    root: &Path,
    targets: &BTreeSet<PathBuf>,
    budget: &mut Budget,
    open: impl Fn(&Path) -> io::Result<fs::ReadDir>,
) -> io::Result<Entries> {
    let root = fs::canonicalize(root)?;
    let mut entries = Entries::new();
    let mut pending = vec![Queued {
        identity: identity(&fs::metadata(&root)?),
        path: root.clone(),
    }];
    let mut visited = BTreeSet::new();
    let mut replaced = Replacements::new();
    while let Some(queued) = pending.pop() {
        if !visited.insert(queued.path.clone()) {
            continue;
        }
        budget.reserve()?;
        match list(&queued, &root, targets, budget, &mut pending, &open)? {
            Listed::Children(children) => {
                entries.insert(queued.path, children);
            }
            Listed::Replaced(current) => {
                replace(
                    queued.path,
                    current,
                    &root,
                    targets,
                    &mut pending,
                    &mut replaced,
                )?;
            }
        }
    }
    reconcile(&mut entries, replaced);
    Ok(entries)
}

fn list(
    queued: &Queued,
    root: &Path,
    targets: &BTreeSet<PathBuf>,
    budget: &mut Budget,
    pending: &mut Vec<Queued>,
    open: &impl Fn(&Path) -> io::Result<fs::ReadDir>,
) -> io::Result<Listed> {
    let directory = queued.path.as_path();
    let listing = match open(directory) {
        Ok(listing) => listing,
        Err(error) if moved_before_open(&error) && directory != root => {
            return match kind(directory)? {
                Some(Kind::Directory) => Err(error),
                current => Ok(Listed::Replaced(current)),
            };
        }
        Err(error) => return Err(error),
    };
    let queued_before = pending.len();
    let mut children = BTreeMap::new();
    for entry in listing {
        budget.reserve()?;
        let entry = entry?;
        let path = entry.path();
        let Some(Seen { kind, identity }) = seen(&path)? else {
            continue;
        };
        if !excluded_entry(&path, &kind, root, targets) {
            match &kind {
                Kind::Directory => pending.push(Queued { path, identity }),
                Kind::Link {
                    resolved: Some(resolved),
                    ..
                } => {
                    if let Some(identity) = directory_identity(resolved)? {
                        pending.push(Queued {
                            path: resolved.clone(),
                            identity,
                        });
                    }
                }
                Kind::File | Kind::Link { .. } | Kind::Other => {}
            }
        }
        children.insert(PathBuf::from(entry.file_name()), kind);
    }
    match seen(directory)? {
        Some(Seen {
            kind: Kind::Directory,
            identity,
        }) if identity == queued.identity => Ok(Listed::Children(children)),
        Some(Seen {
            kind: Kind::Directory,
            ..
        }) => Err(io::Error::other(format!(
            "{} was replaced by another directory while it was listed",
            directory.display()
        ))),
        _ if directory == root => Err(io::Error::other(format!(
            "{} was replaced while it was listed",
            directory.display()
        ))),
        current => {
            pending.truncate(queued_before);
            Ok(Listed::Replaced(current.map(|current| current.kind)))
        }
    }
}

fn replace(
    directory: PathBuf,
    current: Option<Kind>,
    root: &Path,
    targets: &BTreeSet<PathBuf>,
    pending: &mut Vec<Queued>,
    replaced: &mut Replacements,
) -> io::Result<()> {
    if let Some(
        link @ Kind::Link {
            resolved: Some(resolved),
            ..
        },
    ) = &current
        && !excluded_entry(&directory, link, root, targets)
        && let Some(identity) = directory_identity(resolved)?
    {
        pending.push(Queued {
            path: resolved.clone(),
            identity,
        });
    }
    replaced.push((directory, current));
    Ok(())
}

fn reconcile(entries: &mut Entries, replaced: Replacements) {
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
}

fn moved_before_open(error: &io::Error) -> bool {
    error.raw_os_error() == Some(libc::ELOOP)
        || matches!(
            error.kind(),
            io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
        )
}

fn identity(metadata: &fs::Metadata) -> Identity {
    (metadata.dev(), metadata.ino())
}

fn kind(path: &Path) -> io::Result<Option<Kind>> {
    Ok(seen(path)?.map(|seen| seen.kind))
}

fn seen(path: &Path) -> io::Result<Option<Seen>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let identity = identity(&metadata);
    let kind = if metadata.is_symlink() {
        let Some(kind) = link_kind(path)? else {
            return Ok(None);
        };
        kind
    } else if metadata.is_dir() {
        Kind::Directory
    } else if metadata.is_file() {
        Kind::File
    } else {
        Kind::Other
    };
    Ok(Some(Seen { kind, identity }))
}

fn link_kind(path: &Path) -> io::Result<Option<Kind>> {
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

fn directory_identity(path: &Path) -> io::Result<Option<Identity>> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_dir().then(|| identity(&metadata))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(in crate::io::audit) fn unresolved_link(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound || error.raw_os_error() == Some(libc::ELOOP)
}
