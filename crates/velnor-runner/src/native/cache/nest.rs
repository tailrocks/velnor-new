// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::ErrorKind;
use std::os::fd::{AsFd, OwnedFd};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
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

fn outer_of(declared: &[PathBuf], index: usize) -> Option<usize> {
    let path = &declared[index];
    declared
        .iter()
        .enumerate()
        .filter(|(_, other)| *other != path && path.starts_with(other))
        .max_by_key(|(_, other)| other.components().count())
        .map(|(outer, _)| outer)
}

fn resolved(path: &Path) -> PathBuf {
    let mut existing = path;
    let mut missing = Vec::new();
    loop {
        if let Ok(base) = fs::canonicalize(existing) {
            return missing.iter().rev().fold(base, |at, part| at.join(part));
        }
        match (existing.parent(), existing.file_name()) {
            (Some(above), Some(part)) => {
                missing.push(part.to_owned());
                existing = above;
            }
            _ => return path.to_path_buf(),
        }
    }
}

fn aliased_outer(declared: &[PathBuf], index: usize) -> Option<(usize, Vec<OsString>)> {
    let path = &declared[index];
    let parent = resolved(path.parent()?);
    let (outer, root) = declared
        .iter()
        .enumerate()
        .filter(|(other, _)| *other != index)
        .map(|(other, root)| (other, resolved(root)))
        .filter(|(_, root)| parent.starts_with(root))
        .max_by_key(|(_, root)| root.components().count())?;
    let below = parent
        .strip_prefix(&root)
        .ok()?
        .iter()
        .map(OsStr::to_owned)
        .collect();
    Some((outer, below))
}

fn parent_of(declared: &[PathBuf], index: usize) -> Result<Parent> {
    let path = &declared[index];
    let Some(outer) = outer_of(declared, index) else {
        return Ok(match aliased_outer(declared, index) {
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
    let parents = (0..declared.len())
        .map(|index| parent_of(declared, index))
        .collect::<Result<Vec<_>>>()?;
    for (start, path) in declared.iter().enumerate() {
        let mut at = start;
        for _ in 0..parents.len() {
            match &parents[at] {
                Parent::Under { outer, .. } => at = *outer,
                Parent::Ready(_) => break,
            }
        }
        anyhow::ensure!(
            matches!(parents[at], Parent::Ready(_)),
            "the declared path {} and another declared path each resolve below the other",
            path.display()
        );
    }
    Ok(parents)
}
