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
use super::inventory::{self, Budget, Entries};
use crate::error::CiError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Cause {
    Changed,
    Unproven,
}

pub(super) struct Freshness {
    targets: BTreeSet<PathBuf>,
    directories: BTreeMap<(PathBuf, BTreeSet<PathBuf>), (SystemTime, Entries)>,
    budget: Budget,
    max_directory_entries: usize,
}

impl Freshness {
    pub(super) fn new(
        workspaces: &[Workspace],
        groups: &[PathBuf],
        max_directory_entries: usize,
    ) -> Result<Self, CiError> {
        let mut targets = inventory::exclusions(workspaces)?;
        for dir in groups {
            let resolved =
                velnor_rust::resolve(dir).map_err(|error| inventory::unresolved(dir, &error))?;
            targets.insert(resolved);
        }
        Ok(Self {
            targets,
            directories: BTreeMap::new(),
            budget: Budget::new(max_directory_entries),
            max_directory_entries,
        })
    }

    pub(super) fn scratch(&self) -> Self {
        Self {
            targets: self.targets.clone(),
            directories: BTreeMap::new(),
            budget: Budget::new(self.max_directory_entries),
            max_directory_entries: self.max_directory_entries,
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
            let scanned = inventory::scan(&root, targets, &mut self.budget)?;
            let entries = inventory::view(&scanned.entries, &root, targets)?;
            let Some(newest) = inventory::newest(&scanned, &entries)? else {
                return Ok(true);
            };
            self.directories.insert(key.clone(), (newest, entries));
        }
        let Some((newest, entries)) = self.directories.get(&key) else {
            return Err(io::Error::other("directory inventory missing after scan"));
        };
        Ok(*newest > built || !inventory::matches(output, path, entries, targets).unwrap_or(false))
    }
}

impl Audit<'_> {
    pub(super) fn changed_inputs(
        &mut self,
        base: &Path,
        paths: &[String],
        built: SystemTime,
        output: &Path,
        declared: bool,
        limit: usize,
    ) -> Result<Vec<(String, Cause)>, CiError> {
        // A cross-target cache without a prior receipt cannot prove its inputs and remains stale.
        let saved = inventory::load(output)
            .ok()
            .filter(|saved| self.freshness.targets.is_subset(&saved.targets));
        let targets = saved.as_ref().map_or_else(
            || self.freshness.targets.clone(),
            |saved| saved.targets.clone(),
        );
        let mut changed = Vec::new();
        for raw in paths {
            if changed.len() == limit {
                break;
            }
            let path = base.join(raw);
            if declared {
                let before = saved.as_ref().and_then(|saved| saved.state(&path));
                match (before, inventory::state(&path).ok()) {
                    (Some(before), Some(current)) if before == current => {}
                    (Some(_), Some(_)) => {
                        changed.push((raw.clone(), Cause::Changed));
                        continue;
                    }
                    _ => {
                        changed.push((raw.clone(), Cause::Unproven));
                        continue;
                    }
                }
            }
            let stale = match fs::metadata(&path) {
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
            if stale {
                changed.push((raw.clone(), Cause::Changed));
            }
        }
        Ok(changed)
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::UNIX_EPOCH;

    use super::*;

    fn snapshot(name: &str) -> (PathBuf, inventory::Scan) {
        let root =
            std::env::temp_dir().join(format!("velnor-freshness-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
        for file in ["kept.txt", "gone.txt"] {
            let path = root.join(file);
            fs::write(&path, b"").unwrap_or_else(|e| unreachable!("{e}"));
            fs::File::open(&path)
                .and_then(|file| file.set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH)))
                .unwrap_or_else(|e| unreachable!("{e}"));
        }
        let mut budget = Budget::new(usize::MAX);
        let scan = inventory::scan(&root, &BTreeSet::new(), &mut budget)
            .unwrap_or_else(|e| unreachable!("{e}"));
        (root, scan)
    }

    #[test]
    fn a_snapshot_whose_entries_all_remain_has_a_newest_file() {
        let (root, scan) = snapshot("control");
        let newest = inventory::newest(&scan, &scan.entries);
        fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
        assert!(matches!(newest, Ok(Some(_))), "{newest:?}");
    }

    #[test]
    fn an_entry_removed_after_the_scan_is_reported_as_vanished_not_as_an_error() {
        let (root, scan) = snapshot("vanished");
        fs::remove_file(root.join("gone.txt")).unwrap_or_else(|e| unreachable!("{e}"));
        let newest = inventory::newest(&scan, &scan.entries);
        fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
        assert!(matches!(newest, Ok(None)), "{newest:?}");
    }

    fn directory_snapshot(name: &str) -> (PathBuf, inventory::Scan) {
        let root = fs::canonicalize(std::env::temp_dir())
            .unwrap_or_else(|e| unreachable!("{e}"))
            .join(format!("velnor-freshness-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let directory = root.join("d");
        fs::create_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(directory.join("f"), b"").unwrap_or_else(|e| unreachable!("{e}"));
        for path in [directory.join("f"), directory.clone(), root.clone()] {
            fs::File::open(&path)
                .and_then(|file| file.set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH)))
                .unwrap_or_else(|e| unreachable!("{e}"));
        }
        let mut budget = Budget::new(usize::MAX);
        let scan = inventory::scan(&root, &BTreeSet::new(), &mut budget)
            .unwrap_or_else(|e| unreachable!("{e}"));
        (root, scan)
    }

    #[test]
    fn a_file_created_after_the_walk_makes_the_snapshot_stale() {
        let (root, scan) = directory_snapshot("created");
        fs::write(root.join("d").join("new"), b"").unwrap_or_else(|e| unreachable!("{e}"));
        let newest = inventory::newest(&scan, &scan.entries);
        fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
        assert!(
            matches!(newest, Ok(None)),
            "a file created after the walk was missed: {newest:?}"
        );
    }

    #[test]
    fn a_directory_rewritten_with_the_same_names_after_the_walk_makes_the_snapshot_stale() {
        let (root, scan) = directory_snapshot("rewritten");
        let directory = root.join("d");
        fs::remove_file(directory.join("f")).unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(directory.join("f"), b"").unwrap_or_else(|e| unreachable!("{e}"));
        fs::File::open(directory.join("f"))
            .and_then(|file| file.set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH)))
            .unwrap_or_else(|e| unreachable!("{e}"));
        let newest = inventory::newest(&scan, &scan.entries);
        fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
        assert!(
            matches!(newest, Ok(None)),
            "a directory rewritten after the walk was accepted as the walked one: {newest:?}"
        );
    }
}
