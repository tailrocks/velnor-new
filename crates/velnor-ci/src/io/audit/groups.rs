// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use velnor_model::RepoFacts;
use velnor_rust::resolve;

use super::super::units::{MetadataInvocation, Workspace, own_workspace, workspace};
use super::options::AuditOptions;
use crate::error::CiError;

use super::inventory::unresolved;

pub(super) fn shared_groups(facts: &RepoFacts) -> Vec<Vec<usize>> {
    facts
        .config
        .workspace_groups
        .iter()
        .map(|group| {
            facts
                .rust_roots
                .iter()
                .enumerate()
                .filter(|(_, root)| group.roots.contains(&root.manifest))
                .map(|(index, _)| index)
                .collect()
        })
        .collect()
}

pub(super) fn group_dirs(facts: &RepoFacts, repository: &Path) -> Vec<PathBuf> {
    facts
        .config
        .workspace_groups
        .iter()
        .map(|group| repository.join(group.target_dir().as_str()))
        .collect()
}

pub(super) fn standalone(facts: &RepoFacts) -> Vec<bool> {
    facts
        .rust_roots
        .iter()
        .map(|root| {
            !facts
                .config
                .workspace_groups
                .iter()
                .any(|group| group.roots.contains(&root.manifest))
        })
        .collect()
}

pub(super) fn scan_dirs(workspaces: &[Workspace]) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    for directory in workspaces.iter().flat_map(Workspace::directories) {
        if !directories.contains(&directory) {
            directories.push(directory);
        }
    }
    directories
}

pub(super) fn root_workspace(
    manifest: &Path,
    invocation: Option<&MetadataInvocation<'_>>,
    step: &Path,
    inherit: bool,
    alone: bool,
    groups: &[PathBuf],
) -> Result<Workspace, CiError> {
    if !inherit {
        return own_workspace(manifest, invocation, step);
    }
    let found = workspace(manifest, invocation, step)?;
    let target = resolve(&found.target_directory)
        .map_err(|error| unresolved(&found.target_directory, &error))?;
    if alone {
        for group in groups {
            let directory = resolve(group).map_err(|error| unresolved(group, &error))?;
            if directory == target {
                return own_workspace(manifest, invocation, step);
            }
        }
    }
    Ok(found)
}

fn holder(facts: &RepoFacts, repository: &Path, location: &Path) -> Result<Option<usize>, CiError> {
    let location = resolve(location).map_err(|error| unresolved(location, &error))?;
    let mut directories = Vec::new();
    for root in &facts.rust_roots {
        let manifest = repository.join(root.manifest.as_str());
        directories.push(resolve(&manifest).map_err(|error| unresolved(&manifest, &error))?);
    }
    Ok(directories
        .iter()
        .enumerate()
        .filter_map(|(index, directory)| {
            let directory = directory.parent()?.to_path_buf();
            location
                .starts_with(&directory)
                .then(|| (directory.components().count(), index))
        })
        .max_by_key(|(depth, _)| *depth)
        .map(|(_, index)| index))
}

pub(super) fn inheriting(
    facts: &RepoFacts,
    repository: &Path,
    location: Option<&Path>,
) -> Result<Vec<bool>, CiError> {
    let standalone = standalone(facts);
    let Some(location) = location else {
        return Ok(vec![false; standalone.len()]);
    };
    let holder = holder(facts, repository, location)?;
    Ok(standalone
        .into_iter()
        .enumerate()
        .map(|(index, alone)| !alone || holder.is_none_or(|holder| holder == index))
        .collect())
}

/// The roots whose Cargo configuration the step directory decides: the root
/// that holds the step and the roots of its workspace group. A root of another
/// group is probed from its own directory.
pub(super) fn stepped(
    facts: &RepoFacts,
    repository: &Path,
    location: &Path,
    inherit: &[bool],
) -> Result<Vec<bool>, CiError> {
    let holder = holder(facts, repository, location)?;
    let groups = shared_groups(facts);
    Ok(inherit
        .iter()
        .enumerate()
        .map(|(index, inherit)| {
            *inherit
                && holder.is_none_or(|holder| {
                    holder == index
                        || groups
                            .iter()
                            .any(|group| group.contains(&holder) && group.contains(&index))
                })
        })
        .collect())
}

pub(super) fn audit_inheriting(
    facts: &RepoFacts,
    repository: &Path,
    directory: &Path,
    options: &AuditOptions,
) -> Result<Vec<bool>, CiError> {
    let owner = options
        .root
        .as_deref()
        .map_or_else(|| directory.to_path_buf(), |root| repository.join(root));
    inheriting(
        facts,
        repository,
        options.inherit_target_dir.then_some(owner.as_path()),
    )
}

pub(super) fn workspaces(
    facts: &RepoFacts,
    repository: &Path,
    step: &Path,
    probe: Option<(&MetadataInvocation<'_>, &Path)>,
    inherit: &[bool],
) -> Result<Vec<Workspace>, CiError> {
    let groups = group_dirs(facts, repository);
    let stepped = match probe {
        Some((_, location)) => stepped(facts, repository, location, inherit)?,
        None => vec![false; inherit.len()],
    };
    let probe = probe.map(|(probe, _)| probe);
    facts
        .rust_roots
        .iter()
        .zip(standalone(facts))
        .zip(inherit.iter().zip(&stepped))
        .map(|((root, alone), (inherit, stepped))| {
            let manifest = repository.join(root.manifest.as_str());
            root_workspace(
                &manifest,
                probe.filter(|_| *stepped),
                step,
                *inherit,
                alone,
                &groups,
            )
        })
        .collect()
}
