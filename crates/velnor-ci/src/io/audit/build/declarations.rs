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
    pub(super) paths: Vec<PathBuf>,
    pub(super) targets: BTreeSet<PathBuf>,
    pub(super) trusted: bool,
}

pub(super) fn declarations(
    repository: &Path,
    facts: &RepoFacts,
    workspaces: &[Workspace],
    location: &Path,
) -> Result<Vec<Declared>, CiError> {
    let mut found = Vec::new();
    let targets = inventory::exclusions(workspaces)?;
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
            let text = fs::read_to_string(&output).map_err(|e| CiError::new(e.to_string()))?;
            let paths: Vec<PathBuf> = bases
                .iter()
                .flat_map(|base| {
                    rerun_if_changed(&text)
                        .into_iter()
                        .map(|path| base.join(path))
                })
                .collect();
            let directories: Vec<PathBuf> = paths
                .iter()
                .filter(|path| fs::metadata(path).is_ok_and(|metadata| metadata.is_dir()))
                .cloned()
                .collect();
            let trusted = trusted(&output, &paths, &directories, &targets);
            let targets = inventory::load(&output)
                .ok()
                .filter(|saved| targets.is_subset(&saved.targets))
                .map_or_else(|| targets.clone(), |saved| saved.targets.clone());
            found.push(Declared {
                output,
                package,
                directories,
                paths,
                targets,
                trusted,
            });
        }
    }
    Ok(found)
}

fn trusted(
    output: &Path,
    paths: &[PathBuf],
    directories: &[PathBuf],
    targets: &BTreeSet<PathBuf>,
) -> bool {
    // The build scans its roots under the same default the audit uses.
    let mut budget = inventory::Budget::new(super::super::options::DEFAULT_MAX_ENTRIES);
    let saved = inventory::load(output).ok();
    let targets = saved
        .as_ref()
        .filter(|saved| targets.is_subset(&saved.targets))
        .map_or(targets, |saved| &saved.targets);
    paths.iter().all(|path| {
        saved
            .as_ref()
            .and_then(|saved| saved.state(path))
            .zip(inventory::state(path).ok())
            .is_some_and(|(before, current)| before == current)
    }) && directories.iter().all(|path| {
        inventory::scan(path, targets, &mut budget)
            .and_then(|entries| inventory::view(&entries, &fs::canonicalize(path)?, targets))
            .and_then(|entries| inventory::matches(output, path, &entries, targets))
            .unwrap_or(false)
    })
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
