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
    pub publishes_to_crates_io: bool,
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
        mise::check(&files.mise_toml, &files.mise_lock)?;
        let mut binaries = Vec::new();
        for binary in &config.binaries {
            let mut found = configs.iter().filter_map(|candidate| {
                candidate
                    .packages
                    .iter()
                    .find(|package| package.name == binary.package)
                    .map(|package| (candidate.root.clone(), package))
            });
            let (root, package) = found
                .next()
                .ok_or_else(|| ReleaseError::BinaryNotReleased {
                    package: binary.package.clone(),
                })?;
            let others: Vec<String> = found.map(|(other, _)| other).collect();
            if !others.is_empty() {
                return Err(ReleaseError::AmbiguousBinary {
                    package: binary.package.clone(),
                    roots: std::iter::once(root).chain(others).collect(),
                });
            }
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
                    publishes_to_crates_io: config.publishes_to_crates_io,
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
