// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use velnor_rust::NESTED_ROOTS_FOLDER;

use crate::cache_prune::is_hash;
use crate::error::CiError;

mod build_dir;
mod built;
mod fill;
#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod limit_tests;
mod metadata;
#[cfg(test)]
mod owner_tests;
mod separation;
mod workspace;

pub(super) use build_dir::refuse_arguments;
pub(super) use metadata::{MetadataInvocation, own_workspace, workspace};
pub(super) use separation::{apart, outside_tracked};
pub(super) use workspace::{Roots, physical, unit_files_apart};

const MAX_TARGET_DEPTH: usize = 64;
/// A target directory after a full build and test run holds one directory per
/// build script and per unit (`build/<crate>-<hash>`, `.fingerprint/<crate>-<hash>`),
/// so a multi-package workspace passes 4096 easily. The bound only stops a
/// hostile or runaway tree, so it sits far above any real one.
const MAX_TARGET_DIRECTORIES: usize = 1_000_000;
/// The largest value `VELNOR_TARGET_MAX_DIRECTORIES` may raise the bound to.
const TARGET_DIRECTORIES_CEILING: usize = 10_000_000;
const MAX_TARGET_DIRECTORIES_VARIABLE: &str = "VELNOR_TARGET_MAX_DIRECTORIES";

#[derive(Deserialize)]
#[serde(from = "workspace::Printed")]
pub(super) struct Workspace {
    pub(super) root: PathBuf,
    pub(super) target_directory: PathBuf,
    pub(super) build_directory: PathBuf,
    pub(super) shared_build: bool,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum UnitFile {
    Fingerprint {
        configuration: String,
        package: String,
        hash: String,
        name: String,
    },
    DepInfo {
        configuration: String,
        hash: Option<String>,
        stem: String,
    },
    Script {
        profile: String,
        package: String,
        hash: String,
    },
    BuildOutput {
        configuration: String,
        package: String,
        hash: String,
    },
    Directory {
        name: String,
        root: bool,
    },
}

pub(super) fn unit_files(target: &Path) -> Result<Vec<(UnitFile, PathBuf)>, CiError> {
    unit_files_within(target, directory_limit()?)
}

fn directory_limit() -> Result<usize, CiError> {
    limit_from(std::env::var_os(MAX_TARGET_DIRECTORIES_VARIABLE).as_deref())
}

fn limit_from(value: Option<&OsStr>) -> Result<usize, CiError> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(MAX_TARGET_DIRECTORIES);
    };
    value
        .to_str()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|limit| (1..=TARGET_DIRECTORIES_CEILING).contains(limit))
        .ok_or_else(|| {
            CiError::new(format!(
                "{MAX_TARGET_DIRECTORIES_VARIABLE} must be a number from 1 to {TARGET_DIRECTORIES_CEILING}, not \"{}\"",
                value.display()
            ))
        })
}

fn unit_files_within(target: &Path, limit: usize) -> Result<Vec<(UnitFile, PathBuf)>, CiError> {
    let Scanned {
        found,
        built,
        artifacts,
    } = scan(target, limit, None)?;
    built::refuse_without_units(target, built, &found)?;
    built::refuse_orphan(target, &found, &artifacts)?;
    Ok(found)
}

struct Scanned {
    found: Vec<(UnitFile, PathBuf)>,
    built: bool,
    artifacts: Vec<PathBuf>,
}

fn scan(target: &Path, limit: usize, skip: Option<&Path>) -> Result<Scanned, CiError> {
    let mut found = Vec::new();
    let mut artifacts = Vec::new();
    let mut pending = vec![(target.to_path_buf(), 0)];
    let mut directories = 1;
    let mut built = false;
    while let Some((directory, depth)) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(CiError::new(format!(
                    "cannot read {}: {e}",
                    directory.display()
                )));
            }
        };
        let mut places_outputs = None;
        for entry in entries {
            let entry = entry
                .map_err(|e| CiError::new(format!("cannot read {}: {e}", directory.display())))?;
            let path = entry.path();
            let kind = entry
                .file_type()
                .map_err(|e| CiError::new(format!("cannot read {}: {e}", path.display())))?;
            if kind.is_dir() && skip == Some(path.as_path()) {
                continue;
            }
            if kind.is_dir() && entry.file_name() != "incremental" {
                if depth == 0 && entry.file_name() == NESTED_ROOTS_FOLDER {
                    // The build folders of the Cargo roots below this one, each audited as its own root.
                    continue;
                }
                if depth >= MAX_TARGET_DEPTH {
                    return Err(CiError::new(format!(
                        "target directory depth exceeds {MAX_TARGET_DEPTH} at {}",
                        path.display()
                    )));
                }
                if directories == limit {
                    return Err(directory_limit_exceeded(&path, limit));
                }
                directories += 1;
                pending.push((path, depth + 1));
            } else if let Some(unit) = path
                .strip_prefix(target)
                .ok()
                .and_then(unit_file)
                .filter(|_| kind.is_file() && !built::generated(&path))
            {
                found.push((unit, path));
            } else if kind.is_file() && built::output(&entry.file_name()) {
                built = true;
                if built::artifact(&entry.file_name())
                    && !built::generated(&path)
                    && *places_outputs
                        .get_or_insert_with(|| built::places_outputs(&directory, target))
                {
                    artifacts.push(path);
                }
            }
        }
    }
    Ok(Scanned {
        found,
        built,
        artifacts,
    })
}

fn directory_limit_exceeded(path: &Path, limit: usize) -> CiError {
    if limit >= TARGET_DIRECTORIES_CEILING {
        return CiError::new(format!(
            "target directory traversal exceeds {limit} directories at {}; reduce or split the target tree",
            path.display()
        ));
    }
    CiError::new(format!(
        "target directory traversal exceeds {limit} directories at {}; raise the bound with {MAX_TARGET_DIRECTORIES_VARIABLE} (at most {TARGET_DIRECTORIES_CEILING}) or delete the stale target directory",
        path.display()
    ))
}

fn unit_file(relative: &Path) -> Option<UnitFile> {
    let parts: Vec<&str> = relative
        .iter()
        .map(|part| part.to_str())
        .collect::<Option<_>>()?;
    let (name, folders) = parts.split_last()?;
    if let ([directory], "CACHEDIR.TAG" | ".rustc_info.json") = (folders, *name) {
        return Some(UnitFile::Directory {
            name: (*directory).to_owned(),
            root: *name == ".rustc_info.json",
        });
    }
    if let Some(unit) = unit_in_new_layout(folders, name) {
        return Some(unit);
    }
    let configuration = |depth: usize| {
        folders
            .len()
            .checked_sub(depth)
            .map(|end| folders[..end].join("/"))
    };
    let unit = folders.last().and_then(|folder| split(folder));
    let parent = folders.len().checked_sub(2).map(|at| folders[at]);
    match (parent, unit) {
        (Some(".fingerprint"), Some((package, hash))) => Some(UnitFile::Fingerprint {
            configuration: configuration(2)?,
            package: package.to_owned(),
            hash: hash.to_owned(),
            name: (*name).to_owned(),
        }),
        (Some("build"), Some((package, hash))) if *name == "output" => {
            Some(UnitFile::BuildOutput {
                configuration: configuration(2)?,
                package: package.to_owned(),
                hash: hash.to_owned(),
            })
        }
        (Some("build"), Some((package, hash))) => {
            name.strip_suffix(".d").and_then(split).and_then(|_| {
                Some(UnitFile::Script {
                    profile: profile(&configuration(2)?).to_owned(),
                    package: package.to_owned(),
                    hash: hash.to_owned(),
                })
            })
        }
        _ => {
            let stem = name.strip_suffix(".d")?;
            let hash = split(stem).map(|(_, hash)| hash.to_owned());
            let folder = folders
                .last()
                .copied()
                .filter(|folder| matches!(*folder, "deps" | "examples"))?;
            let configuration =
                configuration(1).filter(|configuration| !configuration.is_empty())?;
            (hash.is_some() || folder == "deps").then(|| UnitFile::DepInfo {
                configuration,
                hash,
                stem: stem.to_owned(),
            })
        }
    }
}

/// A file of a unit in the layout Cargo 1.100 writes: `<configuration>/build/<package>/<hash>/`
/// with the unit's `fingerprint`, its `out` files (outputs and dep-info), and for the run of a
/// build script its `run` folder, whose `stdout` is what older Cargo wrote to `output`.
fn unit_in_new_layout(folders: &[&str], name: &str) -> Option<UnitFile> {
    let [configuration @ .., "build", package, hash, kind] = folders else {
        return None;
    };
    if configuration.is_empty() || !is_hash(hash) {
        return None;
    }
    let configuration = configuration.join("/");
    let (package, hash) = ((*package).to_owned(), (*hash).to_owned());
    match (*kind, name) {
        ("fingerprint", _) => Some(UnitFile::Fingerprint {
            configuration,
            package,
            hash,
            name: name.to_owned(),
        }),
        ("run", "stdout") => Some(UnitFile::BuildOutput {
            configuration,
            package,
            hash,
        }),
        ("out", _) => {
            let stem = name.strip_suffix(".d")?;
            if stem.starts_with("build_script_") {
                return Some(UnitFile::Script {
                    profile: profile(&configuration).to_owned(),
                    package,
                    hash,
                });
            }
            Some(UnitFile::DepInfo {
                configuration,
                hash: Some(hash),
                stem: stem.to_owned(),
            })
        }
        _ => None,
    }
}

pub(super) fn profile(configuration: &str) -> &str {
    configuration
        .rsplit_once('/')
        .map_or(configuration, |(_, profile)| profile)
}

fn split(stem: &str) -> Option<(&str, &str)> {
    let (name, hash) = stem.rsplit_once('-')?;
    (!name.is_empty()
        && hash.len() == 16
        && hash
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')))
    .then_some((name, hash))
}
