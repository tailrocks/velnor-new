// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use velnor_rust::normalize;

use super::audit::Audit;
use crate::coverage::HOST;
use crate::error::CiError;

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
    },
}

impl UnitFile {
    pub(super) fn configuration(&self) -> Option<&str> {
        match self {
            Self::Fingerprint { configuration, .. } | Self::BuildOutput { configuration, .. } => {
                Some(configuration)
            }
            Self::DepInfo { configuration, .. } => configuration.as_deref(),
            Self::Script { .. } => None,
        }
    }
}

pub(super) fn unit_files(target: &Path) -> Result<Vec<(UnitFile, PathBuf)>, CiError> {
    let mut found = Vec::new();
    walk(target, target, &mut found)?;
    Ok(found)
}

fn walk(
    target: &Path,
    directory: &Path,
    found: &mut Vec<(UnitFile, PathBuf)>,
) -> Result<(), CiError> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(CiError::new(format!(
                "cannot read {}: {e}",
                directory.display()
            )));
        }
    };
    for entry in entries {
        let entry =
            entry.map_err(|e| CiError::new(format!("cannot read {}: {e}", directory.display())))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|e| CiError::new(format!("cannot read {}: {e}", path.display())))?;
        if kind.is_dir() && entry.file_name() != "incremental" {
            walk(target, &path, found)?;
        } else if let Some(unit) = path
            .strip_prefix(target)
            .ok()
            .and_then(unit_file)
            .filter(|_| kind.is_file())
        {
            found.push((unit, path));
        }
    }
    Ok(())
}

fn unit_file(relative: &Path) -> Option<UnitFile> {
    let parts: Vec<&str> = relative
        .iter()
        .map(|part| part.to_str())
        .collect::<Option<_>>()?;
    let (name, folders) = parts.split_last()?;
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
        (Some("build"), Some((package, _))) if *name == "output" => Some(UnitFile::BuildOutput {
            configuration: configuration(2)?,
            package: package.to_owned(),
        }),
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

pub(super) fn apart(workspaces: &[Workspace]) -> Result<(), CiError> {
    let directories: Vec<PathBuf> = workspaces
        .iter()
        .map(|w| {
            fs::canonicalize(&w.target_directory).unwrap_or_else(|_| normalize(&w.target_directory))
        })
        .collect();
    for (outer, holder) in directories.iter().enumerate() {
        for (inner, held) in directories.iter().enumerate() {
            if outer != inner && held.starts_with(holder) {
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

pub(super) fn workspace(manifest: &Path) -> Result<Workspace, CiError> {
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .arg("--manifest-path")
        .arg(manifest)
        .current_dir(manifest.parent().unwrap_or(manifest))
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

pub(super) fn triples(directory: &Path) -> Result<BTreeSet<String>, CiError> {
    let output = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .args(["--print", "target-list"])
        .current_dir(directory)
        .output()
        .map_err(|e| CiError::new(format!("cannot run rustc --print target-list: {e}")))?;
    if !output.status.success() {
        return Err(CiError::new(format!(
            "rustc --print target-list failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect())
}

impl Audit<'_> {
    pub(super) fn script(
        &mut self,
        workspace: &Path,
        index: usize,
        file: &Path,
        profile: String,
        package: String,
    ) -> Result<(), CiError> {
        if let Some((source, built)) = self.dep_info(workspace, file, Some(&package))? {
            if profile == HOST {
                self.built[index]
                    .scripts
                    .insert((package.clone(), source.clone()));
            }
            self.scripts
                .entry((index, profile, package))
                .or_default()
                .entry(source)
                .and_modify(|compiled| *compiled = (*compiled).min(built))
                .or_insert(built);
        }
        Ok(())
    }

    pub(super) fn orphans(&mut self) {
        for ((index, configuration, hash), (package, fingerprint)) in &self.fingerprints {
            let key = |name: &String| (*index, configuration.clone(), name.clone());
            if !self.present.contains(&key(hash))
                && !self.present.contains(&key(&fingerprint.target))
            {
                self.built[*index].orphans.insert((
                    configuration.clone(),
                    package.clone(),
                    fingerprint.clone(),
                ));
            }
        }
    }
}
