// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use velnor_gen::io::Checkout;
use velnor_model::{CompileInputs, RepoFacts};
use velnor_rust::{Located, Owners, locate, resolve};

use super::dep_info::BuildOutputs;
use super::inventory::unresolved;
use super::{Audit, REBUILD};
use crate::coverage::{Built, gaps};
use crate::error::CiError;
use crate::io::units::{UnitFile, Workspace, unit_files_apart};
use crate::platform::Platforms;

pub(super) fn begin<'a>(
    checkout: &'a Checkout,
    facts: &'a RepoFacts,
    workspaces: &[Workspace],
    groups: &[PathBuf],
    max_directory_entries: usize,
) -> Result<Audit<'a>, CiError> {
    let mut members: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, root) in facts.rust_roots.iter().enumerate() {
        for member in &root.members {
            members
                .entry(member.name.to_string())
                .or_default()
                .push(index);
        }
    }
    let freshness = super::freshness::Freshness::new(workspaces, groups, max_directory_entries)?;
    let diagnostic = freshness.scratch();
    Ok(Audit {
        repository: &checkout.root,
        owners: Owners::new(facts, &checkout.view.tracked_paths),
        tracked: &checkout.view.tracked_paths,
        freshness,
        diagnostic,
        fingerprints: BTreeMap::new(),
        foreign: BTreeMap::new(),
        unhashed: Vec::new(),
        runs: BTreeMap::new(),
        placed: BTreeSet::new(),
        rooted: BTreeSet::new(),
        present: BTreeSet::new(),
        scripts: BTreeMap::new(),
        targets: workspaces
            .iter()
            .flat_map(Workspace::directories)
            .filter_map(|directory| match locate(&checkout.root, &directory, "") {
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
            .map(|workspace| workspace.root.clone())
            .collect(),
        members,
        outputs: BuildOutputs::default(),
        found: CompileInputs::new(),
        target_bytes: vec![0; workspaces.len()],
        stale: Vec::new(),
        units: 0,
        built: workspaces.iter().map(|_| Built::default()).collect(),
    })
}

pub(super) fn gathered_files(
    workspaces: &[Workspace],
    shared: &[Vec<usize>],
    groups: &[PathBuf],
    chosen: &[bool],
) -> Result<Vec<(UnitFile, PathBuf, Vec<usize>)>, CiError> {
    let mut scans: BTreeMap<(PathBuf, PathBuf), Scan> = BTreeMap::new();
    for (index, workspace) in workspaces
        .iter()
        .enumerate()
        .filter(|(index, _)| chosen[*index])
    {
        scans
            .entry((
                resolved(&workspace.target_directory)?,
                resolved(&workspace.build_directory)?,
            ))
            .or_insert_with(|| Scan::new(&workspace.target_directory, &workspace.build_directory))
            .indices
            .insert(index);
    }
    for (indices, directory) in shared.iter().zip(groups) {
        if indices.len() < 2 || !chosen[indices[0]] {
            continue;
        }
        let first = &workspaces[indices[0]];
        let build = if first.shares_build_directory() {
            directory
        } else {
            &first.build_directory
        };
        scans
            .entry((resolved(directory)?, resolved(build)?))
            .or_insert_with(|| Scan::new(directory, build))
            .indices
            .extend(indices);
    }
    let mut files: Vec<(UnitFile, PathBuf, Vec<usize>)> = Vec::new();
    for scan in scans.into_values() {
        let indices: Vec<usize> = scan.indices.into_iter().collect();
        for (unit, file) in unit_files_apart(&scan.target, &scan.build)? {
            files.push((unit, file, indices.clone()));
        }
    }
    files.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
    });
    Ok(files)
}

fn resolved(path: &Path) -> Result<PathBuf, CiError> {
    resolve(path).map_err(|error| unresolved(path, &error))
}

struct Scan {
    target: PathBuf,
    build: PathBuf,
    indices: BTreeSet<usize>,
}

impl Scan {
    fn new(target: &Path, build: &Path) -> Self {
        Self {
            target: target.to_path_buf(),
            build: build.to_path_buf(),
            indices: BTreeSet::new(),
        }
    }
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
        "found no current dep-info {}; run this after {REBUILD} in every Cargo root, and build fully or delete each debug directory that holds a test harness of a member",
        gaps.join("; ")
    )))
}
