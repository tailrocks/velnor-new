// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use velnor_rust::{Located, locate};

use super::super::units::Workspace;
use super::Audit;
use super::inventory::{self, Entries, Kind};
use crate::error::CiError;

pub(super) struct Freshness {
    targets: BTreeSet<PathBuf>,
    directories: BTreeMap<(PathBuf, BTreeSet<PathBuf>), (SystemTime, Entries)>,
    remaining: usize,
}

impl Freshness {
    pub(super) fn new(
        workspaces: &[Workspace],
        groups: &[PathBuf],
        max_directory_entries: usize,
    ) -> Self {
        let mut targets = inventory::exclusions(workspaces);
        targets.extend(groups.iter().map(|dir| velnor_rust::resolve(dir)));
        Self {
            targets,
            directories: BTreeMap::new(),
            remaining: max_directory_entries,
        }
    }

    fn changed(
        &mut self,
        path: &Path,
        built: SystemTime,
        output: &Path,
        targets: &BTreeSet<PathBuf>,
    ) -> io::Result<bool> {
        let metadata = fs::metadata(path)?;
        if !metadata.is_dir() {
            return Ok(metadata.modified()? > built);
        }
        let root = fs::canonicalize(path)?;
        let key = (root.clone(), targets.clone());
        if !self.directories.contains_key(&key) {
            let scanned = inventory::scan(&root, targets, &mut self.remaining)?;
            let entries = inventory::view(&scanned, &root, targets)?;
            let mut newest = SystemTime::UNIX_EPOCH;
            for (directory, children) in &entries {
                for (name, kind) in children {
                    let path = directory.join(name);
                    let metadata = match fs::metadata(&path) {
                        Ok(metadata) => metadata,
                        Err(error)
                            if inventory::unresolved_link(&error)
                                && matches!(kind, Kind::Link { resolved: None, .. }) =>
                        {
                            fs::symlink_metadata(&path)?
                        }
                        Err(error) => return Err(error),
                    };
                    if !metadata.is_dir() {
                        newest = newest.max(metadata.modified()?);
                    }
                }
            }
            self.directories.insert(key.clone(), (newest, entries));
        }
        let Some((newest, entries)) = self.directories.get(&key) else {
            return Err(io::Error::other("directory inventory missing after scan"));
        };
        Ok(*newest > built || !inventory::matches(output, path, entries, targets).unwrap_or(false))
    }
}

impl Audit<'_> {
    pub(super) fn changed_since(
        &mut self,
        base: &Path,
        paths: &[String],
        built: SystemTime,
        output: &Path,
        declared: bool,
    ) -> Result<bool, CiError> {
        // A cross-target cache without a prior receipt cannot prove its inputs and remains stale.
        let saved = inventory::load(output)
            .ok()
            .filter(|saved| self.freshness.targets.is_subset(&saved.targets));
        let targets = saved.as_ref().map_or_else(
            || self.freshness.targets.clone(),
            |saved| saved.targets.clone(),
        );
        for raw in paths {
            let path = base.join(raw);
            if declared
                && !saved
                    .as_ref()
                    .and_then(|saved| saved.state(&path))
                    .zip(inventory::state(&path).ok())
                    .is_some_and(|(before, current)| before == current)
            {
                return Ok(true);
            }
            let changed = match fs::metadata(&path) {
                Ok(metadata)
                    if metadata.is_dir() && self.excluded_tracked_inputs(&path, &targets)? =>
                {
                    true
                }
                Ok(_) => self
                    .freshness
                    .changed(&path, built, output, &targets)
                    .map_err(|error| failed(&path, &error))?,
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

    fn excluded_tracked_inputs(
        &self,
        directory: &Path,
        targets: &BTreeSet<PathBuf>,
    ) -> Result<bool, CiError> {
        let root = fs::canonicalize(directory).map_err(|error| failed(directory, &error))?;
        Ok(self.tracked.iter().any(|tracked| {
            let path = self.repository.join(tracked.as_str());
            path.starts_with(&root)
                && (inventory::excluded(&path, &root, targets)
                    || fs::canonicalize(&path)
                        .is_ok_and(|canonical| inventory::excluded(&canonical, &root, targets)))
        }))
    }
}

fn failed(path: &Path, error: &io::Error) -> CiError {
    CiError::new(format!(
        "cannot establish input freshness for {}: {error}",
        path.display()
    ))
}
