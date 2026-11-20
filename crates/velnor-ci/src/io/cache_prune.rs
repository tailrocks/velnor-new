// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::cache::{directory_size, entries};
use crate::cache_prune::{Category, UsedUnits, is_executable};
use crate::error::CiError;

/// Directories of the build directory that no later build reads back.
const SCRATCH: [(&str, Category); 5] = [
    ("doc", Category::Documentation),
    ("nextest", Category::Scratch),
    ("tmp", Category::Scratch),
    ("package", Category::Scratch),
    ("criterion", Category::Scratch),
];

#[derive(Debug, Default)]
pub(super) struct Removed(BTreeMap<Category, u64>);

impl Removed {
    fn take(&mut self, category: Category, path: &Path) -> Result<(), CiError> {
        let (size, removal) = if path.is_dir() {
            (directory_size(path)?, fs::remove_dir_all(path))
        } else {
            let size = fs::metadata(path)
                .map_err(|e| CiError::new(format!("cannot read {}: {e}", path.display())))?
                .len();
            (size, fs::remove_file(path))
        };
        removal.map_err(|e| CiError::new(format!("cannot remove {}: {e}", path.display())))?;
        *self.0.entry(category).or_default() += size;
        Ok(())
    }

    pub(super) fn describe(&self) -> String {
        let parts: Vec<_> = self
            .0
            .iter()
            .filter(|(_, bytes)| **bytes > 0)
            .map(|(category, bytes)| format!("{} {bytes}", category.label()))
            .collect();
        if parts.is_empty() {
            "nothing else".to_owned()
        } else {
            parts.join(", ")
        }
    }
}

/// Removes from `target` what a later build rebuilds anyway or never reads: documentation,
/// scratch directories, executables and the workspace's own artifacts. With `used`, also
/// every dependency unit the run did not use. Dependency libraries, build script outputs and
/// their fingerprints stay, so a build that restores the result finds its dependencies fresh.
pub(super) fn prune(target: &Path, used: Option<&UsedUnits>) -> Result<Removed, CiError> {
    let mut removed = Removed::default();
    for entry in entries(target)? {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        reject_symlink(&entry)?;
        if !path.is_dir() || name.starts_with('.') {
            continue;
        }
        if let Some((_, category)) = SCRATCH.iter().find(|(scratch, _)| *scratch == name) {
            removed.take(*category, &path)?;
        } else if is_profile(&path) {
            prune_profile(&path, used, &mut removed)?;
        } else {
            for child in entries(&path)? {
                reject_symlink(&child)?;
                let child_path = child.path();
                if child_path.is_dir() && is_profile(&child_path) {
                    prune_profile(&child_path, used, &mut removed)?;
                } else if child_path.is_dir() && child.file_name() == "doc" {
                    removed.take(Category::Documentation, &child_path)?;
                }
            }
        }
    }
    Ok(removed)
}

fn reject_symlink(entry: &fs::DirEntry) -> Result<(), CiError> {
    let kind = entry.file_type().map_err(|e| CiError::new(e.to_string()))?;
    if kind.is_symlink() {
        return Err(CiError::new("cache target contains a symlink"));
    }
    Ok(())
}

fn is_profile(dir: &Path) -> bool {
    dir.join(".fingerprint").is_dir() && dir.join("deps").is_dir()
}

fn prune_profile(
    profile: &Path,
    used: Option<&UsedUnits>,
    removed: &mut Removed,
) -> Result<(), CiError> {
    for entry in entries(profile)? {
        reject_symlink(&entry)?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = path.is_dir();
        match (name.as_str(), is_dir) {
            ("deps", true) => prune_deps(&path, used, removed)?,
            (".fingerprint", true) => {
                prune_units(&path, used, Category::Fingerprints, removed)?;
            }
            ("build", true) => prune_units(&path, used, Category::BuildScripts, removed)?,
            ("examples", true) => removed.take(Category::Executables, &path)?,
            (_, true) if name.ends_with(".dSYM") => removed.take(Category::Executables, &path)?,
            (".cargo-lock", false) => {}
            (_, false) => removed.take(Category::Executables, &path)?,
            (_, true) => {}
        }
    }
    Ok(())
}

fn prune_units(
    dir: &Path,
    used: Option<&UsedUnits>,
    category: Category,
    removed: &mut Removed,
) -> Result<(), CiError> {
    let Some(used) = used else { return Ok(()) };
    for entry in entries(dir)? {
        reject_symlink(&entry)?;
        if used.unused(&entry.file_name().to_string_lossy()) {
            removed.take(category, &entry.path())?;
        }
    }
    Ok(())
}

fn prune_deps(dir: &Path, used: Option<&UsedUnits>, removed: &mut Removed) -> Result<(), CiError> {
    for entry in entries(dir)? {
        reject_symlink(&entry)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let category = if is_executable(&name) {
            Category::Executables
        } else if used.is_some_and(|used| used.unused(&name)) {
            Category::Dependencies
        } else {
            continue;
        };
        removed.take(category, &entry.path())?;
    }
    Ok(())
}
