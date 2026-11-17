// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use velnor_model::RepoPath;
use velnor_rust::{Located, locate};

use super::super::units::Workspace;
use super::Audit;
use crate::error::CiError;

const INPUT_PATH_LIMIT: usize = 100_000;

pub(super) struct Freshness {
    targets: BTreeSet<PathBuf>,
    newest: BTreeMap<PathBuf, SystemTime>,
    remaining: usize,
}

impl Freshness {
    pub(super) fn new(workspaces: &[Workspace]) -> Result<Self, CiError> {
        let mut excluded = BTreeSet::new();
        for target in workspaces.iter().flat_map(|workspace| {
            [
                workspace.workspace_root.join("target"),
                workspace.target_directory.clone(),
            ]
        }) {
            match fs::canonicalize(&target) {
                Ok(target) => {
                    excluded.insert(target);
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(failed(&target, &error)),
            }
        }
        Ok(Self {
            targets: excluded,
            newest: BTreeMap::new(),
            remaining: INPUT_PATH_LIMIT,
        })
    }

    fn newest(
        &mut self,
        root: &Path,
        repository: &Path,
        tracked: &BTreeSet<RepoPath>,
    ) -> io::Result<SystemTime> {
        let key = fs::canonicalize(root)?;
        if let Some(newest) = self.newest.get(&key) {
            return Ok(*newest);
        }
        self.reserve()?;
        let include_targets = self.excluded(&key);
        let mut pending = vec![root.to_path_buf()];
        let mut directories = BTreeSet::new();
        let mut newest = SystemTime::UNIX_EPOCH;
        while let Some(path) = pending.pop() {
            let canonical = fs::canonicalize(&path)?;
            let metadata = fs::metadata(&canonical)?;
            if metadata.is_dir() && !include_targets && self.excluded(&canonical) {
                if tracked_below(repository, tracked, &path)
                    || tracked_below(repository, tracked, &canonical)
                {
                    return Err(io::Error::other("target directory contains tracked inputs"));
                }
                continue;
            }
            if metadata.is_dir() {
                if !directories.insert(canonical) {
                    continue;
                }
                let mut excluded_child = false;
                for entry in fs::read_dir(path)? {
                    self.reserve()?;
                    let child = entry?.path();
                    excluded_child |= !include_targets && self.excluded(&fs::canonicalize(&child)?);
                    pending.push(child);
                }
                if excluded_child {
                    continue;
                }
            }
            newest = newest.max(metadata.modified()?);
        }
        self.newest.insert(key, newest);
        Ok(newest)
    }

    fn excluded(&self, canonical: &Path) -> bool {
        self.targets
            .iter()
            .any(|target| canonical.starts_with(target))
    }

    fn reserve(&mut self) -> io::Result<()> {
        self.remaining = self
            .remaining
            .checked_sub(1)
            .ok_or_else(|| io::Error::other("audit freshness scan limit exceeded"))?;
        Ok(())
    }
}

impl Audit<'_> {
    pub(super) fn changed_since(
        &mut self,
        base: &Path,
        paths: &[String],
        built: SystemTime,
    ) -> Result<bool, CiError> {
        for raw in paths {
            let path = base.join(raw);
            let changed = match fs::metadata(&path) {
                Ok(_) => {
                    self.freshness
                        .newest(&path, self.repository, self.tracked)
                        .map_err(|error| failed(&path, &error))?
                        > built
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    match locate(self.repository, base, raw) {
                        Located::Inside(path) => self
                            .tracked
                            .range(path.clone()..)
                            .take_while(|tracked| tracked.as_str().starts_with(path.as_str()))
                            .any(|tracked| Path::new(tracked.as_str()).starts_with(path.as_str())),
                        Located::Repository => true,
                        Located::Outside => false,
                    }
                }
                Err(error) => return Err(failed(&path, &error)),
            };
            if changed {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn failed(path: &Path, error: &io::Error) -> CiError {
    CiError::new(format!(
        "cannot establish input freshness for {}: {error}",
        path.display()
    ))
}

fn tracked_below(repository: &Path, tracked: &BTreeSet<RepoPath>, path: &Path) -> bool {
    match locate(repository, path, "") {
        Located::Inside(path) => tracked
            .range(path.clone()..)
            .take_while(|tracked| tracked.as_str().starts_with(path.as_str()))
            .any(|tracked| Path::new(tracked.as_str()).starts_with(path.as_str())),
        Located::Repository => true,
        Located::Outside => repository.starts_with(path),
    }
}
