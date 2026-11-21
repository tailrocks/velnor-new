// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use velnor_rust::{Located, locate};

use super::super::restore_record::Record;
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
    record: Rc<Record>,
}

impl Freshness {
    pub(super) fn new(
        workspaces: &[Workspace],
        groups: &[PathBuf],
        max_directory_entries: usize,
        record: Rc<Record>,
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
            record,
        })
    }

    pub(super) fn scratch(&self) -> Self {
        Self {
            targets: self.targets.clone(),
            directories: BTreeMap::new(),
            budget: Budget::new(self.max_directory_entries),
            max_directory_entries: self.max_directory_entries,
            record: Rc::clone(&self.record),
        }
    }

    fn changed(
        &mut self,
        path: &Path,
        built: SystemTime,
        output: &Path,
        targets: &BTreeSet<PathBuf>,
    ) -> io::Result<bool> {
        self.changed_with(path, built, output, targets, &|| {})
    }

    fn changed_with(
        &mut self,
        path: &Path,
        built: SystemTime,
        output: &Path,
        targets: &BTreeSet<PathBuf>,
        between: &dyn Fn(),
    ) -> io::Result<bool> {
        let link = fs::symlink_metadata(path)?;
        between();
        let metadata = fs::metadata(path)?;
        if !same_inode(&link, &fs::symlink_metadata(path)?) {
            return Ok(true);
        }
        let record = Rc::clone(&self.record);
        if link.file_type().is_symlink()
            && changed_since(&link, built, false, || vouched(&record, path, &link))
        {
            return Ok(true);
        }
        if !metadata.is_dir() {
            return Ok(changed_since(&metadata, built, true, || {
                fs::canonicalize(path).is_ok_and(|target| vouched(&record, &target, &metadata))
            }));
        }
        let root = fs::canonicalize(path)?;
        let key = (root.clone(), targets.clone());
        if !self.directories.contains_key(&key) {
            let scanned = inventory::scan(&root, targets, &mut self.budget)?;
            let entries = inventory::view(&scanned.entries, &root, targets)?;
            let Some(newest) = inventory::newest(&scanned, &entries, &self.record)? else {
                return Ok(true);
            };
            self.directories.insert(key.clone(), (newest, entries));
        }
        let Some((newest, entries)) = self.directories.get(&key) else {
            return Err(io::Error::other("directory inventory missing after scan"));
        };
        Ok(
            *newest >= built
                || !inventory::matches(output, path, entries, targets).unwrap_or(false),
        )
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

fn changed_since(
    metadata: &fs::Metadata,
    built: SystemTime,
    keeps_modified: bool,
    vouched: impl FnOnce() -> bool,
) -> bool {
    let (Ok(modified), Some(inode)) = (metadata.modified(), change_time(metadata)) else {
        return true;
    };
    let (modified, inode) = (modified >= built, inode >= built);
    (modified && keeps_modified) || ((modified || inode) && !vouched())
}

fn change_time(metadata: &fs::Metadata) -> Option<SystemTime> {
    let seconds = u64::try_from(metadata.ctime()).ok()?;
    let nanos = u32::try_from(metadata.ctime_nsec()).unwrap_or(0);
    Some(SystemTime::UNIX_EPOCH + Duration::new(seconds, nanos))
}

fn vouched(record: &Record, path: &Path, metadata: &fs::Metadata) -> bool {
    record.vouches_entry(path, (metadata.ctime(), metadata.ctime_nsec()))
}

fn same_inode(first: &fs::Metadata, second: &fs::Metadata) -> bool {
    (first.dev(), first.ino(), first.ctime(), first.ctime_nsec())
        == (
            second.dev(),
            second.ino(),
            second.ctime(),
            second.ctime_nsec(),
        )
        && (first.mtime(), first.mtime_nsec()) == (second.mtime(), second.mtime_nsec())
}

fn failed(path: &Path, error: &io::Error) -> CiError {
    CiError::new(format!(
        "cannot establish input freshness for {}: {error}",
        path.display()
    ))
}

#[cfg(test)]
mod tests;
