// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! A target directory that holds the output of a build but no unit file in a layout this helper
//! reads. Reading it as "no units" would let every verdict that rests on the units (the audit,
//! the inputs lock, the build-script inventories) pass over nothing.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::{UnitFile, split};
use crate::cache_prune::is_hash;
use crate::error::CiError;

/// A file only a build leaves: a compiled library, its metadata, dep-info, or a unit's marker.
pub(super) fn output(name: &OsStr) -> bool {
    let path = Path::new(name);
    path.extension()
        .is_some_and(|extension| matches!(extension.to_str(), Some("rlib" | "rmeta" | "d")))
        || name == "invoked.timestamp"
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

pub(super) fn artifact(name: &OsStr) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| matches!(extension.to_str(), Some("rlib" | "rmeta" | "d")))
}

pub(super) fn refuse_orphan(
    target: &Path,
    found: &[(UnitFile, PathBuf)],
    artifacts: &[PathBuf],
) -> Result<(), CiError> {
    let owned = owned_names(found);
    match artifacts.iter().find(|artifact| !owns(&owned, artifact)) {
        Some(orphan) => Err(CiError::new(format!(
            "found no units in {} that own {}; unknown cargo layout?",
            target.display(),
            orphan.display()
        ))),
        None => Ok(()),
    }
}

fn owned_names(found: &[(UnitFile, PathBuf)]) -> HashSet<(PathBuf, String)> {
    let mut owned = HashSet::new();
    for (unit, path) in found {
        let UnitFile::DepInfo { stem, .. } = unit else {
            continue;
        };
        for scope in scopes(path) {
            owned.insert((scope, crate_name(stem)));
        }
    }
    owned
}

fn owns(owned: &HashSet<(PathBuf, String)>, artifact: &Path) -> bool {
    let names = names(artifact);
    scopes(artifact).into_iter().any(|scope| {
        names
            .iter()
            .any(|name| owned.contains(&(scope.clone(), name.clone())))
    })
}

fn names(artifact: &Path) -> Vec<String> {
    let Some(stem) = artifact.file_stem().and_then(OsStr::to_str) else {
        return Vec::new();
    };
    let bare = stem.strip_prefix("lib");
    let stems: Vec<&str> = match artifact.extension().and_then(OsStr::to_str) {
        Some("d") => [Some(stem), bare].into_iter().flatten().collect(),
        _ => bare.into_iter().collect(),
    };
    stems
        .into_iter()
        .flat_map(|stem| [crate_name(stem), stem.replace('-', "_")])
        .collect()
}

fn crate_name(stem: &str) -> String {
    split(stem).map_or(stem, |(name, _)| name).replace('-', "_")
}

fn named(path: Option<&Path>) -> Option<&str> {
    path?.file_name()?.to_str()
}

fn unit_out_configuration(directory: &Path) -> Option<&Path> {
    let unit = directory.parent();
    let build = unit.and_then(Path::parent).and_then(Path::parent);
    (named(Some(directory)) == Some("out")
        && named(unit).is_some_and(is_hash)
        && named(build) == Some("build"))
    .then(|| build.and_then(Path::parent))
    .flatten()
}

fn scopes(file: &Path) -> Vec<PathBuf> {
    let Some(directory) = file.parent() else {
        return Vec::new();
    };
    if let Some(configuration) = unit_out_configuration(directory) {
        return vec![configuration.to_path_buf()];
    }
    let mut scopes = vec![directory.to_path_buf()];
    if matches!(named(Some(directory)), Some("deps" | "examples"))
        && let Some(configuration) = directory.parent()
    {
        scopes.push(configuration.to_path_buf());
    }
    scopes
}

pub(super) fn generated(file: &Path) -> bool {
    file.ancestors().skip(1).any(script_out_dir)
}

fn script_out_dir(out: &Path) -> bool {
    let Some(unit) = out.parent().filter(|_| named(Some(out)) == Some("out")) else {
        return false;
    };
    let folder = named(Some(unit));
    let build = |directory: Option<&Path>| named(directory) == Some("build");
    folder.is_some_and(|name| split(name).is_some()) && build(unit.parent())
        || folder.is_some_and(is_hash)
            && build(unit.parent().and_then(Path::parent))
            && unit.join("run").is_dir()
}

pub(super) fn places_outputs(directory: &Path, target: &Path) -> bool {
    let configuration = |directory: &Path| {
        directory.join(".cargo-lock").is_file() || directory.join(".fingerprint").is_dir()
    };
    directory == target
        || configuration(directory)
        || matches!(named(Some(directory)), Some("deps" | "examples"))
            && directory.parent().is_some_and(configuration)
        || unit_out_configuration(directory).is_some()
}
