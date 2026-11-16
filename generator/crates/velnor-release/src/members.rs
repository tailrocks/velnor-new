// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{RepoPath, RustMetadata, RustPackage};

use crate::collisions::check_effective_tags;
use crate::error::ReleaseError;
use crate::plan::{manifest_of, read_all};
use crate::registry::check_publish_registries;
use crate::release_plz::{RootConfig, read, valid_crates_io_name, valid_package_name};
use crate::tag::{TagPattern, render};

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

pub(crate) fn inherited_packages(config: &RootConfig, metadata: Option<&Metadata>) -> Vec<String> {
    let Some((_, workspace)) = metadata.and_then(|metadata| owner(&config.root, metadata)) else {
        return Vec::new();
    };
    workspace
        .members
        .iter()
        .map(|member| member.name.as_str())
        .filter(|_| !config.workspace_tags.is_empty())
        .filter(|name| config.releases(name))
        .filter(|name| config.packages.iter().all(|package| package.name != *name))
        .map(str::to_owned)
        .collect()
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
        let inherited: Vec<&str> = inherited(config, workspace)
            .map(|member| member.name.as_str())
            .collect();
        if let Some(name) = inherited.iter().find(|name| {
            !valid_package_name(name) || (config.publishes(name) && !valid_crates_io_name(name))
        }) {
            return Err(ReleaseError::InvalidPackageName {
                root: config.root.clone(),
                package: (*name).to_owned(),
            });
        }
        let listed: Vec<&str> = config
            .packages
            .iter()
            .map(|package| package.name.as_str())
            .collect();
        let unlisted: Vec<String> = inherited
            .iter()
            .filter(|name| !listed.contains(name))
            .map(|name| (*name).to_owned())
            .collect();
        check_workspace_tag_clashes(config, &unlisted)?;
        for name in listed.into_iter().chain(inherited.iter().copied()) {
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
    check_effective_tags(&effective_tags(configs, metadata))
}

fn check_workspace_tag_clashes(
    config: &RootConfig,
    unlisted: &[String],
) -> Result<(), ReleaseError> {
    if let Some((pattern, packages)) = config.workspace_tags.iter().find_map(|pattern| {
        let mut rendered: Vec<(String, TagPattern, bool)> = unlisted
            .iter()
            .filter_map(|name| Some((name.clone(), render(pattern.as_str(), name)?, true)))
            .collect();
        rendered.extend(
            config
                .packages
                .iter()
                .filter(|package| config.releases(&package.name))
                .map(|package| (package.name.clone(), package.tag_pattern.clone(), false)),
        );
        let mut clashing: Vec<String> = rendered
            .iter()
            .enumerate()
            .filter(|(index, (_, tag, inherited))| {
                rendered
                    .iter()
                    .enumerate()
                    .any(|(other, (_, rival, rival_inherited))| {
                        other != *index
                            && (*inherited || *rival_inherited)
                            && tag.may_collide(rival)
                    })
            })
            .map(|(_, (name, _, _))| name.clone())
            .collect();
        clashing.sort();
        (!clashing.is_empty()).then(|| (pattern.as_str().to_owned(), clashing))
    }) {
        return Err(ReleaseError::DuplicateTagPattern { pattern, packages });
    }
    Ok(())
}

pub(crate) fn inherited<'a>(
    config: &'a RootConfig,
    workspace: &'a RustMetadata,
) -> impl Iterator<Item = &'a RustPackage> {
    workspace
        .members
        .iter()
        .filter(|_| !config.workspace_tags.is_empty())
        .filter(|member| config.releases(member.name.as_str()))
        .filter(|member| {
            !config
                .packages
                .iter()
                .any(|package| package.name == member.name.as_str())
        })
}

pub(crate) fn inherited_tag(
    config: &RootConfig,
    workspace: &RustMetadata,
    package: &str,
) -> Option<TagPattern> {
    inherited(config, workspace)
        .find(|member| member.name.as_str() == package)
        .and_then(|_| config.workspace_tags.first())
        .map(|pattern| pattern.instantiate(package))
}

fn effective_tags(configs: &[RootConfig], metadata: &Metadata) -> Vec<(String, TagPattern)> {
    let mut tags = Vec::new();
    for config in configs {
        tags.extend(
            config
                .packages
                .iter()
                .map(|package| (package.name.clone(), package.tag_pattern.clone())),
        );
        if let Some((_, workspace)) = owner(&config.root, metadata) {
            tags.extend(inherited(config, workspace).filter_map(|member| {
                let name = member.name.as_str();
                inherited_tag(config, workspace, name).map(|pattern| (name.to_owned(), pattern))
            }));
        }
    }
    tags
}

pub(crate) fn settle_publishing(configs: &mut [RootConfig], metadata: &Metadata) {
    for config in configs {
        if let Some((_, workspace)) = owner(&config.root, metadata) {
            let unlisted = inherited(config, workspace).next().is_some();
            config.settle_publishing(unlisted);
        }
    }
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

pub fn root_publishes_to_crates_io(
    root: &str,
    text: &str,
    metadata: Option<&Metadata>,
) -> Result<bool, String> {
    let mut configs = [read(root, text).map_err(|error| error.to_string())?];
    if let Some(metadata) = metadata {
        settle_publishing(&mut configs, metadata);
    }
    Ok(configs[0].publishes_to_crates_io)
}
