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
    Ready(OwnedFd),
    Under { outer: usize, below: Vec<OsString> },
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

fn outer_of(declared: &HashMap<&Path, usize>, path: &Path) -> Option<usize> {
    path.ancestors()
        .skip(1)
        .find_map(|ancestor| declared.get(ancestor).copied())
}

fn walk(
    roots: &HashMap<Identity, usize>,
    path: &Path,
    index: usize,
) -> Result<Option<(usize, Vec<OsString>)>> {
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
        if let Some(&outer) = roots.get(&here).filter(|outer| **outer != index) {
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

fn parent_of(
    declared: &[PathBuf],
    texts: &HashMap<&Path, usize>,
    roots: &HashMap<Identity, usize>,
    index: usize,
) -> Result<Parent> {
    let path = &declared[index];
    let Some(outer) = outer_of(texts, path) else {
        return Ok(match walk(roots, path, index)? {
            Some((outer, below)) => Parent::Under { outer, below },
            None => Parent::Ready(open_parent(path)?),
        });
    };
    let below = path
        .parent()
        .and_then(|parent| parent.strip_prefix(&declared[outer]).ok())
        .context("place a declared path under another declared path")?
        .iter()
        .map(OsStr::to_owned)
        .collect();
    Ok(Parent::Under { outer, below })
}

pub(super) fn parents_of(declared: &[PathBuf]) -> Result<Vec<Parent>> {
    let texts: HashMap<&Path, usize> = declared
        .iter()
        .enumerate()
        .map(|(index, path)| (path.as_path(), index))
        .collect();
    let roots: HashMap<Identity, usize> = declared
        .iter()
        .enumerate()
        .filter_map(|(index, path)| identity(path).map(|id| (id, index)))
        .collect();
    let parents = (0..declared.len())
        .map(|index| parent_of(declared, &texts, &roots, index))
        .collect::<Result<Vec<_>>>()?;
    for (start, path) in declared.iter().enumerate() {
        let mut at = start;
        for _ in 0..parents.len() {
            match &parents[at] {
                Parent::Under { outer, .. } => at = *outer,
                Parent::Ready(_) => break,
            }
        }
        ensure!(
            matches!(parents[at], Parent::Ready(_)),
            "the declared path {} and another declared path each resolve below the other",
            path.display()
        );
    }
    Ok(parents)
}
