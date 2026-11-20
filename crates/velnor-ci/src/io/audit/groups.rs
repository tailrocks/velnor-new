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
    for workspace in workspaces {
        if !directories.contains(&workspace.target_directory) {
            directories.push(workspace.target_directory.clone());
        }
    }
    directories
}

pub(super) fn root_workspace(
    manifest: &Path,
    invocation: Option<&MetadataInvocation<'_>>,
    inherit: bool,
    alone: bool,
    groups: &[PathBuf],
) -> Result<Workspace, CiError> {
    if !inherit {
        return own_workspace(manifest, invocation);
    }
    let found = workspace(manifest, invocation)?;
    let target = resolve(&found.target_directory)
        .map_err(|error| unresolved(&found.target_directory, &error))?;
    if alone {
        for group in groups {
            let directory = resolve(group).map_err(|error| unresolved(group, &error))?;
            if directory == target {
                return own_workspace(manifest, invocation);
            }
        }
    }
    Ok(found)
}

/// Which Cargo roots read the ambient `CARGO_TARGET_DIR`, for a command or
/// audit that runs at `location`. Every grouped root reads it, since its group
/// shares one target directory. A standalone root reads it only when it holds
/// `location`: the environment belongs to the step that runs there, and a
/// second standalone root reading it would report the same target as the first
/// and be refused by `apart`. With no holder, every standalone root reads it.
pub(super) fn inheriting(
    facts: &RepoFacts,
    repository: &Path,
    location: Option<&Path>,
) -> Result<Vec<bool>, CiError> {
    let standalone = standalone(facts);
    let Some(location) = location else {
        return Ok(vec![false; standalone.len()]);
    };
    let location = resolve(location).map_err(|error| unresolved(location, &error))?;
    let mut directories = Vec::new();
    for root in &facts.rust_roots {
        let manifest = repository.join(root.manifest.as_str());
        directories.push(resolve(&manifest).map_err(|error| unresolved(&manifest, &error))?);
    }
    let holder = directories
        .iter()
        .enumerate()
        .filter_map(|(index, directory)| {
            let directory = directory.parent()?.to_path_buf();
            location
                .starts_with(&directory)
                .then(|| (directory.components().count(), index))
        })
        .max_by_key(|(depth, _)| *depth)
        .map(|(_, index)| index);
    Ok(standalone
        .into_iter()
        .enumerate()
        .map(|(index, alone)| !alone || holder.is_none_or(|holder| holder == index))
        .collect())
}

/// `inputs audit` reads the target of the root `--root` names, or of the
/// directory it runs in when no root is named.
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
    inherit: &[bool],
) -> Result<Vec<Workspace>, CiError> {
    let groups = group_dirs(facts, repository);
    facts
        .rust_roots
        .iter()
        .zip(standalone(facts))
        .zip(inherit)
        .map(|((root, alone), inherit)| {
            let manifest = repository.join(root.manifest.as_str());
            root_workspace(&manifest, None, *inherit, alone, &groups)
        })
        .collect()
}
