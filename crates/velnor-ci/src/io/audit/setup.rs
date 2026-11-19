// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use velnor_gen::io::Checkout;
use velnor_model::{CompileInputs, RepoFacts};
use velnor_rust::{Located, Owners, locate};

use super::Audit;
use crate::coverage::{Built, gaps};
use crate::error::CiError;
use crate::io::units::{UnitFile, Workspace, unit_files};
use crate::platform::Platforms;

pub(super) fn begin<'a>(
    checkout: &'a Checkout,
    facts: &'a RepoFacts,
    workspaces: &[Workspace],
    groups: &[PathBuf],
    max_directory_entries: usize,
) -> Audit<'a> {
    let mut members: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, root) in facts.rust_roots.iter().enumerate() {
        for member in &root.members {
            members
                .entry(member.name.to_string())
                .or_default()
                .push(index);
        }
    }
    Audit {
        repository: &checkout.root,
        owners: Owners::new(facts, &checkout.view.tracked_paths),
        tracked: &checkout.view.tracked_paths,
        freshness: super::freshness::Freshness::new(workspaces, groups, max_directory_entries),
        fingerprints: BTreeMap::new(),
        names: BTreeMap::new(),
        runs: BTreeMap::new(),
        placed: BTreeSet::new(),
        rooted: BTreeSet::new(),
        present: BTreeSet::new(),
        scripts: BTreeMap::new(),
        targets: workspaces
            .iter()
            .filter_map(|w| match locate(&checkout.root, &w.target_directory, "") {
                Located::Inside(target) => Some(target),
                Located::Repository | Located::Outside => None,
            })
            .chain(
                groups
                    .iter()
                    .filter_map(|dir| match locate(&checkout.root, dir, "") {
                        Located::Inside(target) => Some(target),
                        Located::Repository | Located::Outside => None,
                    }),
            )
            .collect(),
        roots: workspaces
            .iter()
            .map(|workspace| workspace.workspace_root.clone())
            .collect(),
        members,
        found: CompileInputs::new(),
        target_bytes: vec![0; workspaces.len()],
        stale: Vec::new(),
        units: 0,
        built: workspaces.iter().map(|_| Built::default()).collect(),
    }
}

pub(super) fn gathered_files(
    workspaces: &[Workspace],
    shared: &[Vec<usize>],
    groups: &[PathBuf],
    chosen: &[bool],
) -> Result<Vec<(UnitFile, PathBuf, Vec<usize>)>, CiError> {
    let mut files: Vec<(UnitFile, PathBuf, Vec<usize>)> = Vec::new();
    for (index, workspace) in workspaces
        .iter()
        .enumerate()
        .filter(|(index, _)| chosen[*index])
    {
        files.extend(
            unit_files(&workspace.target_directory)?
                .into_iter()
                .map(|(unit, file)| (unit, file, vec![index])),
        );
    }
    for (indices, directory) in shared.iter().zip(groups) {
        if indices.len() < 2 || !chosen[indices[0]] {
            continue;
        }
        files.extend(
            unit_files(directory)?
                .into_iter()
                .map(|(unit, file)| (unit, file, indices.clone())),
        );
    }
    files.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
    });
    Ok(files)
}

pub(super) fn complete(
    facts: &RepoFacts,
    chosen: &[bool],
    effective: &[PathBuf],
    built: &[Built],
    platforms: &[Platforms],
) -> Result<(), CiError> {
    let gaps: Vec<String> = facts
        .rust_roots
        .iter()
        .zip(chosen)
        .zip(effective)
        .zip(built)
        .zip(platforms)
        .filter(|((((_, chosen), _), _), _)| **chosen)
        .flat_map(|((((root, _), directory), built), platform)| {
            gaps(root, &facts.unowned, built, platform)
                .into_iter()
                .map(|gap| {
                    format!(
                        "in {} for {}",
                        directory.join(&gap.configuration).display(),
                        gap.missing.join(", ")
                    )
                })
        })
        .collect();
    if gaps.is_empty() {
        return Ok(());
    }
    Err(CiError::new(format!(
        "found no current dep-info {}; run this after velnor-ci inputs build -- cargo build --workspace --all-targets in every Cargo root, and build fully or delete each debug directory that holds a test harness of a member",
        gaps.join("; ")
    )))
}
