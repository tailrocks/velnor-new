// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    MemberDependency, PackageName, RepoPath, RepoView, RustCiConfig, RustMember, RustMetadata,
    RustPackage, RustRoot,
};

use crate::config::excluded;
use crate::error::DetectError;

pub(crate) const CARGO_LOCK: RepoPath = RepoPath::from_static("Cargo.lock");

type Candidate<'a> = (&'a RepoPath, &'a RustMetadata);

pub(crate) fn rust_roots(
    view: &RepoView,
    config: &RustCiConfig,
) -> Result<Vec<RustRoot>, DetectError> {
    let candidates: Vec<Candidate<'_>> = view
        .rust_metadata
        .iter()
        .filter(|(manifest, metadata)| !metadata.members.is_empty() && !excluded(config, manifest))
        .collect();
    if candidates.is_empty() {
        return Err(DetectError::NoRustRoot);
    }
    let owner: BTreeMap<&RepoPath, &RepoPath> = candidates
        .iter()
        .flat_map(|(root, metadata)| metadata.members.iter().map(|m| (&m.manifest, *root)))
        .collect();
    let mut named: BTreeMap<&PackageName, Vec<&RepoPath>> = BTreeMap::new();
    for package in candidates
        .iter()
        .flat_map(|(_, metadata)| &metadata.members)
    {
        named
            .entry(&package.name)
            .or_default()
            .push(&package.manifest);
    }
    candidates
        .iter()
        .map(|(manifest, metadata)| {
            let lockfile = manifest.sibling(&CARGO_LOCK);
            if !view.tracked_paths.contains(&lockfile) {
                return Err(DetectError::MissingLockfile {
                    path: lockfile,
                    manifest: (*manifest).clone(),
                });
            }
            Ok(root(manifest, metadata, &owner, &named))
        })
        .collect()
}

fn root(
    manifest: &RepoPath,
    metadata: &RustMetadata,
    owner: &BTreeMap<&RepoPath, &RepoPath>,
    named: &BTreeMap<&PackageName, Vec<&RepoPath>>,
) -> RustRoot {
    let depends_on: BTreeSet<RepoPath> = metadata
        .members
        .iter()
        .flat_map(|package| &package.dependencies)
        .filter_map(|dependency| owner.get(dependency.manifest.as_ref()?))
        .filter(|root| **root != manifest)
        .map(|root| (*root).clone())
        .collect();
    let mut members: Vec<RustMember> = metadata
        .members
        .iter()
        .map(|package| member(package, named))
        .collect();
    members.sort_by(|a, b| a.name.cmp(&b.name));
    RustRoot {
        manifest: manifest.clone(),
        members,
        depends_on: depends_on.into_iter().collect(),
    }
}

fn member(package: &RustPackage, named: &BTreeMap<&PackageName, Vec<&RepoPath>>) -> RustMember {
    RustMember {
        name: package.name.clone(),
        manifest: package.manifest.clone(),
        directory: package.manifest.parent(),
        dependencies: package
            .dependencies
            .iter()
            .flat_map(|dependency| {
                let manifests: Vec<RepoPath> = match &dependency.manifest {
                    Some(manifest) => vec![manifest.clone()],
                    None => named
                        .get(&dependency.name)
                        .into_iter()
                        .flatten()
                        .map(|manifest| (*manifest).clone())
                        .collect(),
                };
                manifests.into_iter().map(|manifest| MemberDependency {
                    name: dependency.name.clone(),
                    kind: dependency.kind,
                    manifest,
                })
            })
            .collect(),
        targets: package.targets.clone(),
    }
}
