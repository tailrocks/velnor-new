// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! A target directory that holds the output of a build but no unit file in a layout this helper
//! reads. Reading it as "no units" would let every verdict that rests on the units (the audit,
//! the inputs lock, the build-script inventories) pass over nothing.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::{UnitFile, split};
use crate::error::CiError;

/// A file only a build leaves: a compiled library, its metadata, dep-info, or a unit's marker.
pub(super) fn output(name: &OsStr) -> bool {
    let path = Path::new(name);
    path.extension()
        .is_some_and(|extension| matches!(extension.to_str(), Some("rlib" | "rmeta" | "d")))
        || name == "invoked.timestamp"
}

pub(super) fn run_folder(file: &Path) -> bool {
    file.parent()
        .and_then(Path::file_name)
        .is_some_and(|folder| folder == "run")
}

/// Which built outputs belong to a unit Cargo recorded: those in the very directory of a unit
/// file, and the final artifacts Cargo places in the configuration folder (or its `examples`
/// folder) for a library or binary whose intermediate file is in `deps` or a build-layout `out`
/// folder.
pub(super) struct Owners<'a> {
    unit_directories: HashSet<&'a Path>,
    produced: HashSet<(&'a Path, &'a str, String)>,
}

impl<'a> Owners<'a> {
    pub(super) fn new(found: &'a [(UnitFile, PathBuf)], outputs: &'a [PathBuf]) -> Self {
        let units = found
            .iter()
            .filter(|(unit, _)| !matches!(unit, UnitFile::Directory { .. }))
            .map(|(_, path)| path.as_path());
        let unit_directories = units.clone().filter_map(Path::parent).collect();
        let produced = units
            .chain(outputs.iter().map(PathBuf::as_path))
            .filter_map(intermediate)
            .collect();
        Self {
            unit_directories,
            produced,
        }
    }

    pub(super) fn owns(&self, output: &Path) -> bool {
        let Some(directory) = output.parent() else {
            return false;
        };
        if self.unit_directories.contains(directory) {
            return true;
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

fn stem_and_extension(file: &Path) -> Option<(&str, &str)> {
    let extension = file.extension()?.to_str()?;
    matches!(extension, "rlib" | "d").then_some(())?;
    Some((file.file_stem()?.to_str()?, extension))
}

fn intermediate(file: &Path) -> Option<(&Path, &str, String)> {
    let (stem, extension) = stem_and_extension(file)?;
    let (name, _) = split(stem)?;
    let directory = file.parent()?;
    let configuration = match directory.file_name()?.to_str()? {
        "deps" => directory.parent()?,
        "out" => {
            let build = directory.parent()?.parent()?.parent()?;
            build.ends_with("build").then(|| build.parent())??
        }
        _ => return None,
    };
    Some((configuration, extension, name.replace('-', "_")))
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
