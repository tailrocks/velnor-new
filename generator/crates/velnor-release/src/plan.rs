// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::error::ReleaseError;
use crate::mise;
use crate::release_plz::{RootConfig, read};
use crate::tag::TagPattern;
use crate::velnor_config::parse;

#[derive(Debug, Clone, Default)]
pub struct ReleaseFiles {
    pub release_toml: Option<String>,
    pub release_plz_toml: BTreeMap<String, String>,
    pub mise_toml: String,
    pub mise_lock: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootPlan {
    pub root: String,
    pub pr_branch_prefix: String,
    pub packages: Vec<(String, TagPattern)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryPlan {
    pub package: String,
    pub root: String,
    pub platforms: Vec<String>,
    pub tag_pattern: TagPattern,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasePlan {
    pub roots: Vec<RootPlan>,
    pub after: Vec<String>,
    pub binaries: Vec<BinaryPlan>,
}

fn check_unique(configs: &[RootConfig]) -> Result<(), ReleaseError> {
    let mut tags: BTreeMap<&TagPattern, Vec<String>> = BTreeMap::new();
    let mut prefixes: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for config in configs {
        prefixes
            .entry(&config.pr_branch_prefix)
            .or_default()
            .push(config.root.clone());
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
    if let Some((prefix, roots)) = prefixes.into_iter().find(|(_, roots)| roots.len() > 1) {
        return Err(ReleaseError::DuplicatePrBranchPrefix {
            prefix: prefix.to_owned(),
            roots,
        });
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
        mise::check(&files.mise_toml, &files.mise_lock)?;
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
            binaries.push(BinaryPlan {
                package: binary.package.clone(),
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
                    packages: config
                        .packages
                        .into_iter()
                        .map(|package| (package.name, package.tag_pattern))
                        .collect(),
                })
                .collect(),
            after: config.after,
            binaries,
        })
    }
}
