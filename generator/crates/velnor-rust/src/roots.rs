// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    MemberDependency, PackageName, RepoPath, RepoView, RustCiConfig, RustMember, RustMetadata,
    RustPackage, RustRoot,
};

use crate::config::excluded;
use crate::error::DetectError;
use crate::toml_file::table;

pub(crate) const CARGO_LOCK: RepoPath = RepoPath::from_static("Cargo.lock");

type Candidate<'a> = (&'a RepoPath, &'a RustMetadata);
type Targets = BTreeMap<PackageName, BTreeSet<RepoPath>>;

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
    let mut named: Targets = BTreeMap::new();
    for package in candidates
        .iter()
        .flat_map(|(_, metadata)| &metadata.members)
    {
        named
            .entry(package.name.clone())
            .or_default()
            .insert(package.manifest.clone());
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
            let mut targets = named.clone();
            for (name, target) in patches(view.files.get(*manifest), manifest) {
                targets.entry(name).or_default().insert(target);
            }
            Ok(root(manifest, metadata, &owner, &targets))
        })
        .collect()
}

fn root(
    manifest: &RepoPath,
    metadata: &RustMetadata,
    owner: &BTreeMap<&RepoPath, &RepoPath>,
    targets: &Targets,
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
        .map(|package| member(package, targets))
        .collect();
    members.sort_by(|a, b| a.name.cmp(&b.name));
    RustRoot {
        manifest: manifest.clone(),
        members,
        depends_on: depends_on.into_iter().collect(),
    }
}

fn patches(manifest_text: Option<&Vec<u8>>, manifest: &RepoPath) -> Vec<(PackageName, RepoPath)> {
    let Some(table) = manifest_text.and_then(|bytes| table(bytes).ok()) else {
        return Vec::new();
    };
    table
        .get("patch")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flat_map(|sources| sources.values().filter_map(toml::Value::as_table))
        .flatten()
        .filter_map(|(name, patch)| {
            let folder = patch.get("path")?.as_str()?;
            let target = within(manifest.parent().as_ref(), folder)?;
            Some((PackageName::new(name.as_str()).ok()?, target))
        })
        .collect()
}

fn within(base: Option<&RepoPath>, relative: &str) -> Option<RepoPath> {
    let mut segments: Vec<&str> =
        base.map_or_else(Vec::new, |base| base.as_str().split('/').collect());
    for segment in relative.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }
    segments.push("Cargo.toml");
    RepoPath::new(segments.join("/")).ok()
}

fn member(package: &RustPackage, targets: &Targets) -> RustMember {
    RustMember {
        name: package.name.clone(),
        manifest: package.manifest.clone(),
        directory: package.manifest.parent(),
        dependencies: package
            .dependencies
            .iter()
            .flat_map(|dependency| {
                let manifests: BTreeSet<RepoPath> = match &dependency.manifest {
                    Some(manifest) => BTreeSet::from([manifest.clone()]),
                    None => targets.get(&dependency.name).cloned().unwrap_or_default(),
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
