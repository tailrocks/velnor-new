// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::Platform;

use crate::error::ReleaseError;
use crate::hazards::credential_provider;
use crate::mise;
use crate::release_plz::{Package, RootConfig, read};
use crate::tag::TagPattern;
use crate::velnor_config::parse;

#[derive(Debug, Clone, Default)]
pub struct ReleaseFiles {
    pub release_toml: Option<String>,
    pub release_plz_toml: BTreeMap<String, String>,
    pub mise_tomls: Vec<String>,
    pub cargo_configs: BTreeMap<String, String>,
    pub bin_targets: Option<BTreeSet<(String, String)>>,
    pub mise_lock: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootPlan {
    pub root: String,
    pub pr_branch_prefix: String,
    pub publishes_to_crates_io: bool,
    pub packages: Vec<(String, TagPattern)>,
    pub workspace_tags: Vec<TagPattern>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryPlan {
    pub package: String,
    pub bin: String,
    pub root: String,
    pub platforms: Vec<Platform>,
    pub tag_pattern: TagPattern,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasePlan {
    pub roots: Vec<RootPlan>,
    pub after: Vec<String>,
    pub binaries: Vec<BinaryPlan>,
    pub release_plz_tool: String,
}

fn check_unique(configs: &[RootConfig]) -> Result<(), ReleaseError> {
    let mut tags: BTreeMap<&TagPattern, Vec<String>> = BTreeMap::new();
    for config in configs {
        for package in &config.packages {
            tags.entry(&package.tag_pattern)
                .or_default()
                .push(package.name.clone());
        }
    }
    if let Some((pattern, packages)) = tags.into_iter().find(|(_, names)| names.len() > 1) {
        return Err(ReleaseError::DuplicateTagPattern {
            pattern: pattern.as_str().to_owned(),
            packages,
        });
    }
    let all: Vec<(&str, &Package)> = configs
        .iter()
        .flat_map(|config| config.packages.iter().map(|p| (config.root.as_str(), p)))
        .collect();
    for (index, (root, package)) in all.iter().enumerate() {
        for (other_root, other) in &all[index + 1..] {
            if other.name == package.name {
                return Err(ReleaseError::DuplicatePackageName {
                    package: package.name.clone(),
                    roots: vec![(*root).to_owned(), (*other_root).to_owned()],
                });
            }
            if other.tag_pattern.may_collide(&package.tag_pattern) {
                return Err(ReleaseError::TagsMayCollide {
                    first: package.tag_pattern.as_str().to_owned(),
                    second: other.tag_pattern.as_str().to_owned(),
                });
            }
        }
    }
    let prefixes: Vec<(&str, &str)> = configs
        .iter()
        .map(|config| (config.pr_branch_prefix.as_str(), config.root.as_str()))
        .collect();
    for (index, (prefix, root)) in prefixes.iter().enumerate() {
        if let Some((other, other_root)) = prefixes[index + 1..]
            .iter()
            .find(|(other, _)| other.starts_with(prefix) || prefix.starts_with(other))
        {
            let shorter = if prefix.len() <= other.len() {
                prefix
            } else {
                other
            };
            return Err(ReleaseError::DuplicatePrBranchPrefix {
                prefix: (*shorter).to_owned(),
                roots: vec![(*root).to_owned(), (*other_root).to_owned()],
            });
        }
    }
    Ok(())
}

impl ReleasePlan {
    pub fn read(files: &ReleaseFiles) -> Result<ReleasePlan, ReleaseError> {
        let config = parse(files.release_toml.as_deref())?;
        let mut configs = Vec::new();
        for root in &config.roots {
            let text = files
                .release_plz_toml
                .get(root)
                .ok_or_else(|| ReleaseError::MissingReleasePlzToml { root: root.clone() })?;
            configs.push(read(root, text)?);
        }
        check_unique(&configs)?;
        let release_plz_tool = mise::check(&files.mise_tomls, &files.mise_lock)?;
        if configs.iter().any(|config| config.publishes_to_crates_io) {
            for (file, text) in &files.cargo_configs {
                credential_provider(file, text)?;
            }
        }
        let mut binaries = Vec::new();
        for binary in &config.binaries {
            let (root, package) = configs
                .iter()
                .find_map(|candidate| {
                    candidate
                        .packages
                        .iter()
                        .find(|package| package.name == binary.package)
                        .map(|package| (candidate.root.clone(), package))
                })
                .ok_or_else(|| ReleaseError::BinaryNotReleased {
                    package: binary.package.clone(),
                })?;
            if let Some(targets) = &files.bin_targets
                && !targets.contains(&(binary.package.clone(), binary.bin.clone()))
            {
                return Err(ReleaseError::BinaryTargetMissing {
                    package: binary.package.clone(),
                    bin: binary.bin.clone(),
                });
            }
            binaries.push(BinaryPlan {
                package: binary.package.clone(),
                bin: binary.bin.clone(),
                root,
                platforms: binary.platforms.clone(),
                tag_pattern: package.tag_pattern.clone(),
            });
        }
        Ok(ReleasePlan {
            roots: configs
                .into_iter()
                .map(|config| RootPlan {
                    root: config.root,
                    pr_branch_prefix: config.pr_branch_prefix,
                    publishes_to_crates_io: config.publishes_to_crates_io,
                    packages: config
                        .packages
                        .into_iter()
                        .map(|package| (package.name, package.tag_pattern))
                        .collect(),
                    workspace_tags: config.workspace_tags,
                })
                .collect(),
            after: config.after,
            binaries,
            release_plz_tool,
        })
    }
}
