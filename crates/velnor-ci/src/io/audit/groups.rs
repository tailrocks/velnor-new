// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

use velnor_model::RepoFacts;
use velnor_rust::normalize;

use super::super::units::{MetadataInvocation, Workspace, own_workspace, workspace};
use crate::error::CiError;

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

fn resolved(path: &Path) -> PathBuf {
    let path = normalize(path);
    let mut missing = Vec::new();
    let mut existing = path.as_path();
    loop {
        if let Ok(real) = fs::canonicalize(existing) {
            return missing
                .iter()
                .rev()
                .fold(real, |real, part| real.join(part));
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                missing.push(name.to_owned());
                existing = parent;
            }
            _ => return path,
        }
    }
}

pub(super) fn root_workspace(
    manifest: &Path,
    invocation: Option<&MetadataInvocation<'_>>,
    alone: bool,
    groups: &[PathBuf],
) -> Result<Workspace, CiError> {
    let found = workspace(manifest, invocation)?;
    let target = resolved(&found.target_directory);
    if alone && groups.iter().any(|group| resolved(group) == target) {
        return own_workspace(manifest, invocation);
    }
    Ok(found)
}

/// Which Cargo roots take the build's inherited `CARGO_TARGET_DIR`.
///
/// A build runs in one Cargo root, so the variable names that root's target
/// directory and says nothing about a standalone root the build does not touch.
/// Every grouped root keeps inheriting it because a group shares one directory.
pub(super) fn inheriting(
    facts: &RepoFacts,
    repository: &Path,
    location: Option<&Path>,
) -> Vec<bool> {
    let location = location.map(resolved);
    let depth = |root: &velnor_model::RustRoot| {
        let directory = root
            .manifest
            .as_str()
            .rsplit_once('/')
            .map_or("", |(parent, _)| parent);
        let directory = resolved(&repository.join(directory));
        location
            .as_ref()
            .filter(|location| location.starts_with(&directory))
            .map(|_| directory.components().count())
    };
    let building = facts
        .rust_roots
        .iter()
        .zip(standalone(facts))
        .filter(|(_, alone)| *alone)
        .filter_map(|(root, _)| depth(root))
        .max();
    facts
        .rust_roots
        .iter()
        .zip(standalone(facts))
        .map(|(root, alone)| !alone || building.is_none() || depth(root) == building)
        .collect()
}

pub(super) fn workspaces(
    facts: &RepoFacts,
    repository: &Path,
    inherit_target_dir: bool,
    inheriting: &[bool],
) -> Result<Vec<Workspace>, CiError> {
    let groups = group_dirs(facts, repository);
    facts
        .rust_roots
        .iter()
        .zip(standalone(facts))
        .zip(inheriting)
        .map(|((root, alone), inherits)| {
            let manifest = repository.join(root.manifest.as_str());
            if !inherit_target_dir || !inherits {
                return own_workspace(&manifest, None);
            }
            root_workspace(&manifest, None, alone, &groups)
        })
        .collect()
}
