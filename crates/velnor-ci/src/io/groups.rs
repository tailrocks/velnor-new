// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

use velnor_model::RepoFacts;
use velnor_rust::normalize;

use super::units::{MetadataInvocation, Workspace, own_workspace, workspace};
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

pub(super) fn workspaces(facts: &RepoFacts, repository: &Path) -> Result<Vec<Workspace>, CiError> {
    let groups = group_dirs(facts, repository);
    facts
        .rust_roots
        .iter()
        .zip(standalone(facts))
        .map(|(root, alone)| {
            root_workspace(
                &repository.join(root.manifest.as_str()),
                None,
                alone,
                &groups,
            )
        })
        .collect()
}
