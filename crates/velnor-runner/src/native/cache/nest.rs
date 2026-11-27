// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{HashMap, VecDeque};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::ErrorKind;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, ensure};
use rustix::fs::{CWD, Mode, OFlags, openat};

use super::descent::enter;

pub(super) enum Parent {
    Unresolved,
    Resolving,
    Ready(OwnedFd),
}

pub(super) enum Link {
    Free(OwnedFd),
    Below { outer: usize, below: Vec<OsString> },
}

fn open_parent(path: &Path) -> Result<OwnedFd> {
    let parent = path
        .parent()
        .context("find the parent of a declared path")?;
    let mut existing = parent;
    let mut missing = Vec::new();
    let base = loop {
        match fs::canonicalize(existing) {
            Ok(base) => break base,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let (Some(above), Some(part)) = (existing.parent(), existing.file_name()) else {
                    return Err(error).with_context(|| format!("resolve {}", path.display()));
                };
                missing.push(part);
                existing = above;
            }
            Err(error) => {
                return Err(error).with_context(|| format!("resolve {}", path.display()));
            }
        }
    };
    let mut directory = openat(
        CWD,
        &base,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .with_context(|| format!("open {}", base.display()))?;
    for part in missing.into_iter().rev() {
        directory = enter(directory.as_fd(), part)?;
    }
    Ok(directory)
}

type Identity = (u64, u64);

const LINK_LIMIT: usize = 40;

fn identity(path: &Path) -> Option<Identity> {
    let meta = fs::symlink_metadata(path).ok()?;
    Some((meta.dev(), meta.ino()))
}

fn names(path: &Path) -> impl DoubleEndedIterator<Item = OsString> + '_ {
    path.components().filter_map(|part| match part {
        Component::Normal(name) => Some(name.to_owned()),
        Component::ParentDir => Some(OsString::from("..")),
        _ => None,
    })
}

pub(super) struct Nesting {
    texts: HashMap<PathBuf, usize>,
    identities: HashMap<Identity, usize>,
}

impl Nesting {
    pub(super) fn new(declared: &[PathBuf]) -> Self {
        let mut nesting = Self {
            texts: declared.iter().cloned().zip(0..).collect(),
            identities: HashMap::new(),
        };
        for index in 0..declared.len() {
            nesting.note(declared, index);
        }
        nesting
    }

    pub(super) fn note(&mut self, declared: &[PathBuf], index: usize) {
        if let Some(here) = identity(&declared[index]) {
            self.identities.insert(here, index);
        }
    }

    fn root_at(&self, declared: &[PathBuf], here: Identity, index: usize) -> Option<usize> {
        self.identities
            .get(&here)
            .copied()
            .filter(|&outer| outer != index && identity(&declared[outer]) == Some(here))
    }

    fn outer_of(&self, path: &Path) -> Option<usize> {
        path.ancestors()
            .skip(1)
            .find_map(|ancestor| self.texts.get(ancestor).copied())
    }

    fn walk(&self, declared: &[PathBuf], index: usize) -> Result<Option<(usize, Vec<OsString>)>> {
        let path = &declared[index];
        let Some(parent) = path.parent() else {
            return Ok(None);
        };
        let mut at = PathBuf::from("/");
        let mut queue: VecDeque<OsString> = names(parent).collect();
        let mut links = 0;
        while let Some(part) = queue.pop_front() {
            if part == ".." {
                at.pop();
                continue;
            }
            let next = at.join(&part);
            let Some(here) = identity(&next) else {
                return Ok(None);
            };
            if let Some(outer) = self.root_at(declared, here, index) {
                let below: Vec<OsString> = queue.into_iter().collect();
                ensure!(
                    below.iter().all(|name| name != ".."),
                    "the declared path {} leaves another declared path through a link",
                    path.display()
                );
                return Ok(Some((outer, below)));
            }
            match fs::read_link(&next) {
                Ok(target) => {
                    links += 1;
                    ensure!(
                        links <= LINK_LIMIT,
                        "resolve {}: too many links",
                        path.display()
                    );
                    if target.is_absolute() {
                        at = PathBuf::from("/");
                    }
                    for name in names(&target).rev() {
                        queue.push_front(name);
                    }
                }
                Err(_) => at = next,
            }
        }
        Ok(None)
    }

    pub(super) fn resolve(&self, declared: &[PathBuf], index: usize) -> Result<Link> {
        let path = &declared[index];
        if let Some(outer) = self.outer_of(path) {
            let below = path
                .parent()
                .and_then(|parent| parent.strip_prefix(&declared[outer]).ok())
                .context("place a declared path under another declared path")?
                .iter()
                .map(OsStr::to_owned)
                .collect();
            return Ok(Link::Below { outer, below });
        }
        Ok(match self.walk(declared, index)? {
            Some((outer, below)) => Link::Below { outer, below },
            None => Link::Free(open_parent(path)?),
        })
    }
}
