// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::cache::{directory_size, entries};
use crate::cache_prune::{Category, UsedUnits, is_executable, is_hash};
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
    (dir.join(".fingerprint").is_dir() && dir.join("deps").is_dir())
        || fs::read_dir(dir.join("build")).is_ok_and(|build| {
            build
                .flatten()
                .any(|package| !unit_dirs(&package.path()).is_empty())
        })
}

/// The unit directories `<package>/<hash>` that Cargo 1.100 keeps below `build/`; none for an
/// older `build/<package>-<hash>` unit.
fn unit_dirs(package: &Path) -> Vec<PathBuf> {
    fs::read_dir(package)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|unit| unit.path().is_dir() && is_hash(&unit.file_name().to_string_lossy()))
        .map(|unit| unit.path())
        .collect()
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
            ("build", true) => prune_build(&path, used, removed)?,
            ("examples", true) => removed.take(Category::Executables, &path)?,
            (_, true) if name.ends_with(".dSYM") => removed.take(Category::Executables, &path)?,
            (".cargo-lock", false) | (_, true) => {}
            (name, false) if name.starts_with(".cargo-") => {}
            (_, false) => removed.take(Category::Executables, &path)?,
        }
    }
    Ok(())
}

/// `build/` holds one directory per build script unit (`<package>-<hash>`), and since Cargo 1.100
/// one per package with a directory per unit of any kind below it (`<package>/<hash>`).
fn prune_build(dir: &Path, used: Option<&UsedUnits>, removed: &mut Removed) -> Result<(), CiError> {
    for entry in entries(dir)? {
        reject_symlink(&entry)?;
        let units = unit_dirs(&entry.path());
        if units.is_empty() {
            if used.is_some_and(|used| used.unused(&entry.file_name().to_string_lossy())) {
                removed.take(Category::BuildScripts, &entry.path())?;
            }
            continue;
        }
        for unit in units {
            prune_unit(&unit, used, removed)?;
        }
    }
    Ok(())
}

fn prune_unit(unit: &Path, used: Option<&UsedUnits>, removed: &mut Removed) -> Result<(), CiError> {
    let ran = unit.join("run").is_dir();
    let unused = used.is_some_and(|used| {
        unit.file_name()
            .is_some_and(|hash| used.unused(&hash.to_string_lossy()))
    });
    for part in entries(unit)? {
        reject_symlink(&part)?;
        let path = part.path();
        let category = match (part.file_name().to_string_lossy().as_ref(), ran) {
            ("fingerprint", _) => Category::Fingerprints,
            ("run", _) | ("out", true) => Category::BuildScripts,
            ("out", false) => Category::Dependencies,
            _ => continue,
        };
        if unused {
            removed.take(category, &path)?;
        } else if !ran && category == Category::Dependencies {
            prune_executables(&path, removed)?;
        }
    }
    if unused {
        fs::remove_dir(unit)
            .map_err(|e| CiError::new(format!("cannot remove {}: {e}", unit.display())))?;
    }
    Ok(())
}

/// The test, bench and binary executables of a unit's `out`; the executable of a build script
/// stays, as it does in `build/<package>-<hash>`.
fn prune_executables(out: &Path, removed: &mut Removed) -> Result<(), CiError> {
    for file in entries(out)? {
        reject_symlink(&file)?;
        let name = file.file_name().to_string_lossy().into_owned();
        if !name.starts_with("build_script_") && is_executable(&name) && file.path().is_file() {
            removed.take(Category::Executables, &file.path())?;
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
