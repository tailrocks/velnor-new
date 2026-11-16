// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{RepoPath, RustMetadata};

use crate::error::ReleaseError;
use crate::plan::{manifest_of, read_all};
use crate::registry::check_publish_registries;
use crate::release_plz::RootConfig;
use crate::tag::PACKAGE_WILDCARD;

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

fn owning<'a>(
    root: &str,
    metadata: &'a Metadata,
) -> Result<(&'a RepoPath, &'a RustMetadata), ReleaseError> {
    owner(root, metadata).ok_or_else(|| ReleaseError::RootWithoutWorkspace {
        root: root.to_owned(),
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
        let (_, workspace) = owning(&config.root, metadata)?;
        let known: BTreeSet<&str> = workspace
            .members
            .iter()
            .map(|member| member.name.as_str())
            .collect();
        if let Some(package) = config
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
            .filter(|name| config.releases(name))
            .copied();
        let listed: Vec<&str> = config
            .packages
            .iter()
            .map(|package| package.name.as_str())
            .collect();
        let unlisted: Vec<String> = inherited
            .clone()
            .filter(|name| !listed.contains(name))
            .map(str::to_owned)
            .collect();
        if unlisted.len() > 1
            && let Some(pattern) = config
                .workspace_tags
                .iter()
                .find(|pattern| !pattern.as_str().contains(PACKAGE_WILDCARD))
        {
            return Err(ReleaseError::DuplicateTagPattern {
                pattern: pattern.as_str().to_owned(),
                packages: unlisted,
            });
        }
        for name in listed.into_iter().chain(inherited) {
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
        let (workspace, known) = owning(&config.root, metadata)?;
        check_publish_registries(config, workspace, known, manifests)?;
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
