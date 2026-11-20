// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use velnor_rust::{NESTED_ROOTS_FOLDER, resolve};

use crate::error::CiError;

mod fill;

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
pub(super) struct Workspace {
    pub(super) workspace_root: PathBuf,
    pub(super) target_directory: PathBuf,
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
        configuration: Option<String>,
        hash: Option<String>,
        stem: String,
    },
    Script {
        profile: String,
        package: String,
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
    limit_from(
        std::env::var(MAX_TARGET_DIRECTORIES_VARIABLE)
            .ok()
            .as_deref(),
    )
}

fn limit_from(value: Option<&str>) -> Result<usize, CiError> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(MAX_TARGET_DIRECTORIES);
    };
    value
        .parse::<usize>()
        .ok()
        .filter(|limit| (1..=TARGET_DIRECTORIES_CEILING).contains(limit))
        .ok_or_else(|| {
            CiError::new(format!(
                "{MAX_TARGET_DIRECTORIES_VARIABLE} must be a number from 1 to {TARGET_DIRECTORIES_CEILING}, not {value:?}"
            ))
        })
}

fn unit_files_within(target: &Path, limit: usize) -> Result<Vec<(UnitFile, PathBuf)>, CiError> {
    let mut found = Vec::new();
    let mut pending = vec![(target.to_path_buf(), 0)];
    let mut directories = 1;
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
        for entry in entries {
            let entry = entry
                .map_err(|e| CiError::new(format!("cannot read {}: {e}", directory.display())))?;
            let path = entry.path();
            let kind = entry
                .file_type()
                .map_err(|e| CiError::new(format!("cannot read {}: {e}", path.display())))?;
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
                    return Err(CiError::new(format!(
                        "target directory traversal exceeds {limit} directories at {}; raise the bound with {MAX_TARGET_DIRECTORIES_VARIABLE} (at most {TARGET_DIRECTORIES_CEILING}) or delete the stale target directory",
                        path.display()
                    )));
                }
                directories += 1;
                pending.push((path, depth + 1));
            } else if let Some(unit) = path
                .strip_prefix(target)
                .ok()
                .and_then(unit_file)
                .filter(|_| kind.is_file())
            {
                found.push((unit, path));
            }
        }
    }
    Ok(found)
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
        (Some("build"), Some((package, _))) => {
            name.strip_suffix(".d").and_then(split).and_then(|_| {
                Some(UnitFile::Script {
                    profile: profile(&configuration(2)?).to_owned(),
                    package: package.to_owned(),
                })
            })
        }
        _ => {
            let stem = name.strip_suffix(".d")?;
            let hash = split(stem).map(|(_, hash)| hash.to_owned());
            let placed = folders
                .last()
                .is_some_and(|folder| matches!(*folder, "deps" | "examples"));
            (hash.is_some() || folders.last() == Some(&"deps")).then(|| UnitFile::DepInfo {
                configuration: configuration(1).filter(|_| placed),
                hash,
                stem: stem.to_owned(),
            })
        }
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

pub(super) fn apart(workspaces: &[Workspace], shared: &[Vec<usize>]) -> Result<(), CiError> {
    let directories: Vec<PathBuf> = workspaces
        .iter()
        .map(|w| {
            resolve(&w.target_directory).map_err(|error| {
                CiError::new(format!(
                    "cannot resolve {}: {error}",
                    w.target_directory.display()
                ))
            })
        })
        .collect::<Result<_, _>>()?;
    for (outer, holder) in directories.iter().enumerate() {
        for (inner, held) in directories.iter().enumerate() {
            if outer == inner
                || !held.starts_with(holder)
                || held.starts_with(holder.join(NESTED_ROOTS_FOLDER))
            {
                continue;
            }
            let grouped = held == holder
                && shared
                    .iter()
                    .any(|group| group.contains(&outer) && group.contains(&inner));
            if !grouped {
                return Err(CiError::new(format!(
                    "the target directory {} of the Cargo root {} holds the target directory {} of the Cargo root {}, so their units cannot be told apart; give each Cargo root its own target directory, outside the others",
                    holder.display(),
                    workspaces[outer].workspace_root.display(),
                    held.display(),
                    workspaces[inner].workspace_root.display()
                )));
            }
        }
    }
    Ok(())
}

pub(super) struct MetadataInvocation<'a> {
    pub(super) executable: &'a str,
    pub(super) arguments: &'a [String],
    pub(super) directory: &'a Path,
}

pub(super) fn workspace(
    manifest: &Path,
    invocation: Option<&MetadataInvocation<'_>>,
) -> Result<Workspace, CiError> {
    metadata(manifest, invocation, false)
}

pub(super) fn own_workspace(
    manifest: &Path,
    invocation: Option<&MetadataInvocation<'_>>,
) -> Result<Workspace, CiError> {
    metadata(manifest, invocation, true)
}

fn metadata(
    manifest: &Path,
    invocation: Option<&MetadataInvocation<'_>>,
    own_target: bool,
) -> Result<Workspace, CiError> {
    let mut command = Command::new(invocation.map_or("cargo", |invocation| invocation.executable));
    if let Some(invocation) = invocation {
        command
            .args(invocation.arguments)
            .current_dir(invocation.directory);
    } else {
        command.current_dir(manifest.parent().unwrap_or(manifest));
    }
    if own_target {
        command.env_remove("CARGO_TARGET_DIR");
    }
    let output = command
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .arg("--manifest-path")
        .arg(manifest)
        .output()
        .map_err(|e| CiError::new(format!("cannot run cargo metadata: {e}")))?;
    if !output.status.success() {
        return Err(CiError::new(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|e| CiError::new(format!("cargo metadata printed no workspace: {e}")))
}
