// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{RepoPath, RustMetadata};

use crate::error::ReleaseError;
use crate::plan::{manifest_of, read_all};
use crate::registry::check_publish_registries;
use crate::release_plz::RootConfig;

type Metadata = BTreeMap<RepoPath, RustMetadata>;

pub(crate) fn owner<'a>(
    root: &str,
    metadata: &'a Metadata,
) -> Option<(&'a RepoPath, &'a RustMetadata)> {
    let manifest = manifest_of(root);
    metadata.iter().find(|(workspace, known)| {
        workspace.as_str() == manifest
            || known
                .members
                .iter()
                .any(|member| member.manifest.as_str() == manifest)
    })
}

pub(crate) fn check_inventory(
    configs: &[RootConfig],
    metadata: Option<&Metadata>,
) -> Result<(), ReleaseError> {
    let Some(metadata) = metadata else {
        return Ok(());
    };
    let mut released: BTreeMap<&str, &str> = BTreeMap::new();
    for config in configs {
        let known: Option<BTreeSet<&str>> = owner(&config.root, metadata).map(|(_, workspace)| {
            workspace
                .members
                .iter()
                .map(|member| member.name.as_str())
                .collect()
        });
        if let Some(known) = &known
            && let Some(package) = config
                .packages
                .iter()
                .find(|package| !known.contains(package.name.as_str()))
        {
            return Err(ReleaseError::PackageNotInRoot {
                root: config.root.clone(),
                package: package.name.clone(),
            });
        }
        let inherited = known
            .iter()
            .filter(|_| !config.workspace_tags.is_empty())
            .flatten()
            .copied();
        let listed = config.packages.iter().map(|package| package.name.as_str());
        for name in listed.chain(inherited) {
            if let Some(first) = released.insert(name, config.root.as_str())
                && first != config.root
            {
                return Err(ReleaseError::DuplicatePackageName {
                    package: name.to_owned(),
                    roots: vec![first.to_owned(), config.root.clone()],
                });
            }
        }
    }
    Ok(())
}

pub(crate) fn check_registries(
    configs: &[RootConfig],
    metadata: &Metadata,
    manifests: &BTreeMap<String, String>,
) -> Result<(), ReleaseError> {
    for config in configs {
        if let Some((workspace, known)) = owner(&config.root, metadata) {
            check_publish_registries(config, workspace, known, manifests)?;
        }
    }
    Ok(())
}

pub fn check_release_manifests(
    files: &[(String, String)],
    metadata: &Metadata,
    manifests: &BTreeMap<String, String>,
) -> Result<(), String> {
    let configs = read_all(files)?;
    check_inventory(&configs, Some(metadata)).map_err(|error| error.to_string())?;
    check_registries(&configs, metadata, manifests).map_err(|error| error.to_string())
}
