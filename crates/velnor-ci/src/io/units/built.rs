// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! A target directory that holds the output of a build but no unit file in a layout this helper
//! reads. Reading it as "no units" would let every verdict that rests on the units (the audit,
//! the inputs lock, the build-script inventories) pass over nothing.

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::{UnitFile, split};
use crate::cache_prune::is_hash;
use crate::error::CiError;

const TIMESTAMP: &str = "invoked.timestamp";

/// A file only a build leaves: a compiled library, its metadata, dep-info, or a unit's marker.
pub(super) fn output(name: &OsStr) -> bool {
    artifact(name) || name == TIMESTAMP
}

pub(super) fn artifact(name: &OsStr) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| matches!(extension.to_str(), Some("rlib" | "rmeta" | "d")))
}

pub(super) fn bookkeeping(file: &Path) -> bool {
    file.file_name().is_some_and(|name| name == TIMESTAMP)
        && file.parent().is_some_and(written_by_cargo)
}

fn written_by_cargo(directory: &Path) -> bool {
    let parent = directory.parent();
    let build = |path: Option<&Path>| named(path) == Some("build");
    match named(Some(directory)) {
        Some("run") => {
            named(parent).is_some_and(is_hash)
                && parent
                    .and_then(Path::parent)
                    .is_some_and(|package| build(package.parent()))
        }
        Some(folder) => split(folder).is_some() && build(parent),
        None => false,
    }
}

fn named(path: Option<&Path>) -> Option<&str> {
    path?.file_name()?.to_str()
}

pub(super) struct Owners<'a> {
    unit_names: HashMap<&'a Path, HashSet<String>>,
    produced: HashSet<(&'a Path, &'a str, String)>,
}

impl<'a> Owners<'a> {
    pub(super) fn new(found: &'a [(UnitFile, PathBuf)], outputs: &'a [PathBuf]) -> Self {
        let units = found
            .iter()
            .filter(|(unit, _)| !matches!(unit, UnitFile::Directory { .. }))
            .map(|(_, path)| path.as_path());
        let mut unit_names: HashMap<&Path, HashSet<String>> = HashMap::new();
        for path in units.clone() {
            let Some(directory) = path.parent() else {
                continue;
            };
            let names = unit_names.entry(directory).or_default();
            if let Some((stem, "d")) = stem_and_extension(path) {
                names.insert(normalized(stem));
            }
        }
        let produced = units
            .chain(outputs.iter().map(PathBuf::as_path))
            .filter_map(intermediate)
            .collect();
        Self {
            unit_names,
            produced,
        }
    }

    pub(super) fn owns(&self, output: &Path) -> bool {
        let Some(directory) = output.parent() else {
            return false;
        };
        if let Some(names) = self.unit_names.get(directory) {
            return unit_name(output).is_some_and(|name| names.contains(&name));
        }
        let configuration = if directory.file_name().is_some_and(|name| name == "examples") {
            directory.parent().unwrap_or(directory)
        } else {
            directory
        };
        let Some((stem, extension)) = stem_and_extension(output) else {
            return false;
        };
        let bare = (extension == "d")
            .then(|| stem.strip_prefix("lib"))
            .flatten();
        [Some(stem), bare].into_iter().flatten().any(|name| {
            self.produced
                .contains(&(configuration, extension, name.replace('-', "_")))
        })
    }
}

fn normalized(stem: &str) -> String {
    split(stem).map_or(stem, |(name, _)| name).replace('-', "_")
}

fn unit_name(file: &Path) -> Option<String> {
    let stem = file.file_stem()?.to_str()?;
    match file.extension()?.to_str()? {
        "d" => Some(normalized(stem)),
        "rlib" | "rmeta" => stem.strip_prefix("lib").map(normalized),
        _ => None,
    }
}

fn stem_and_extension(file: &Path) -> Option<(&str, &str)> {
    let extension = file.extension()?.to_str()?;
    matches!(extension, "rlib" | "d").then_some(())?;
    Some((file.file_stem()?.to_str()?, extension))
}

fn intermediate(file: &Path) -> Option<(&Path, &str, String)> {
    let (stem, extension) = stem_and_extension(file)?;
    let directory = file.parent()?;
    let (configuration, hashed) = match directory.file_name()?.to_str()? {
        "deps" => (directory.parent()?, true),
        "out" => {
            let build = directory.parent()?.parent()?.parent()?;
            (build.ends_with("build").then(|| build.parent())??, false)
        }
        _ => return None,
    };
    let name = match split(stem) {
        Some((name, _)) => name,
        None if hashed => return None,
        None => stem,
    };
    Some((configuration, extension, name.to_owned()))
}

pub(super) fn refuse_orphan(output: Option<&Path>) -> Result<(), CiError> {
    let Some(output) = output else {
        return Ok(());
    };
    let directory = output.parent().unwrap_or(output);
    Err(CiError::new(format!(
        "found no units in {}; unknown cargo layout?",
        directory.display()
    )))
}

pub(super) fn refuse_without_units(
    target: &Path,
    built: bool,
    found: &[(UnitFile, PathBuf)],
) -> Result<(), CiError> {
    let unit = |file: &UnitFile| !matches!(file, UnitFile::Directory { .. });
    if built && !found.iter().any(|(file, _)| unit(file)) {
        return Err(CiError::new(format!(
            "found no units in {}; unknown cargo layout?",
            target.display()
        )));
    }
    Ok(())
}
