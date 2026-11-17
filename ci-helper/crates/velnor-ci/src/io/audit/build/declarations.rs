// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use velnor_model::RepoFacts;
use velnor_rust::rerun_if_changed;

use super::super::super::units::{UnitFile, Workspace, unit_files};
use super::super::inventory;
use crate::error::CiError;

pub(super) struct Declared {
    pub(super) output: PathBuf,
    pub(super) package: String,
    pub(super) directories: Vec<PathBuf>,
    pub(super) trusted: bool,
}

pub(super) fn declarations(
    repository: &Path,
    facts: &RepoFacts,
    workspaces: &[Workspace],
    location: &Path,
) -> Result<Vec<Declared>, CiError> {
    let mut found = Vec::new();
    let targets = inventory::exclusions(workspaces);
    let active = workspaces
        .iter()
        .enumerate()
        .filter(|(_, workspace)| location.starts_with(&workspace.workspace_root))
        .max_by_key(|(_, workspace)| workspace.workspace_root.components().count())
        .map(|(index, _)| index);
    for (_, workspace) in workspaces
        .iter()
        .enumerate()
        .filter(|(index, _)| Some(*index) == active)
    {
        let mut bases: BTreeMap<String, BTreeSet<PathBuf>> = BTreeMap::new();
        for root in &facts.rust_roots {
            if repository.join(root.manifest.as_str()).parent()
                == Some(workspace.workspace_root.as_path())
            {
                for member in &root.members {
                    if let Some(base) = repository.join(member.manifest.as_str()).parent() {
                        bases
                            .entry(member.name.to_string())
                            .or_default()
                            .insert(base.to_path_buf());
                    }
                }
            }
        }
        for (manifest, package) in &facts.unowned {
            if let Some(base) = repository.join(manifest.as_str()).parent() {
                bases
                    .entry(package.name.to_string())
                    .or_default()
                    .insert(base.to_path_buf());
            }
        }
        for (unit, output) in unit_files(&workspace.target_directory)? {
            let UnitFile::BuildOutput { package, .. } = unit else {
                continue;
            };
            let Some(bases) = bases.get(&package) else {
                continue;
            };
            if bases.len() != 1 {
                return Err(CiError::new(format!(
                    "cannot associate build output {} with one package directory",
                    output.display()
                )));
            }
            let Some(base) = bases.first() else {
                continue;
            };
            let text = fs::read_to_string(&output).map_err(|e| CiError::new(e.to_string()))?;
            let directories: Vec<PathBuf> = rerun_if_changed(&text)
                .iter()
                .map(|path| base.join(path))
                .filter(|path| fs::metadata(path).is_ok_and(|metadata| metadata.is_dir()))
                .collect();
            if directories.is_empty() {
                continue;
            }
            let mut remaining = inventory::LIMIT * 10;
            let trusted = directories.iter().all(|path| {
                inventory::scan(path, &targets, &mut remaining)
                    .and_then(|entries| {
                        inventory::view(&entries, &fs::canonicalize(path)?, &targets)
                    })
                    .and_then(|entries| inventory::matches(&output, path, &entries, &targets))
                    .unwrap_or(false)
            });
            found.push(Declared {
                output,
                package,
                directories,
                trusted,
            });
        }
    }
    Ok(found)
}

pub(super) fn invalidate(declared: &[Declared]) -> Result<(), CiError> {
    for unit in declared.iter().filter(|unit| !unit.trusted) {
        fs::remove_file(&unit.output).map_err(|e| {
            CiError::new(format!(
                "cannot invalidate untrusted build output {}: {e}",
                unit.output.display()
            ))
        })?;
    }
    Ok(())
}
