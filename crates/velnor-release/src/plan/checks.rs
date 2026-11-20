// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::error::ReleaseError;
use crate::release_plz::RootConfig;
use crate::tag::{PACKAGE_WILDCARD, TagPattern};
use crate::timeout::{TOKEN_LIFETIME_MINUTES, TOKEN_MARGIN_MINUTES};

use super::{RootPlan, publish_budget_minutes, read_all};

pub(crate) fn check_unique(configs: &[RootConfig]) -> Result<(), ReleaseError> {
    let mut tags: BTreeMap<&TagPattern, Vec<String>> = BTreeMap::new();
    for config in configs {
        for package in config.tag_owners() {
            tags.entry(&package.tag_pattern)
                .or_default()
                .push(package.name.clone());
        }
        for pattern in config
            .workspace_tags
            .iter()
            .filter(|pattern| !pattern.as_str().contains(PACKAGE_WILDCARD))
        {
            tags.entry(pattern)
                .or_default()
                .push(format!("[workspace] of {}", config.root));
        }
    }
    if let Some((pattern, packages)) = tags.into_iter().find(|(_, names)| names.len() > 1) {
        return Err(ReleaseError::DuplicateTagPattern {
            pattern: pattern.as_str().to_owned(),
            packages,
        });
    }
    let mut all: Vec<(&str, &TagPattern, Option<&str>, Option<&str>)> = Vec::new();
    for config in configs {
        let root = config.root.as_str();
        all.extend(config.packages.iter().map(|package| {
            (
                root,
                &package.tag_pattern,
                Some(package.name.as_str()),
                package.group.as_deref(),
            )
        }));
        all.extend(
            config
                .workspace_tags
                .iter()
                .filter(|pattern| !pattern.as_str().contains(PACKAGE_WILDCARD))
                .map(|pattern| (root, pattern, None, None)),
        );
    }
    for (index, (root, pattern, name, group)) in all.iter().enumerate() {
        for (other_root, other_pattern, other_name, other_group) in &all[index + 1..] {
            let same_root = root == other_root;
            if let (Some(name), Some(other_name)) = (name, other_name)
                && name == other_name
            {
                return Err(ReleaseError::DuplicatePackageName {
                    package: (*name).to_owned(),
                    roots: vec![(*root).to_owned(), (*other_root).to_owned()],
                });
            }
            let same_group = group.is_some() && root == other_root && group == other_group;
            let both_listed = name.is_some() && other_name.is_some();
            if !same_group && (both_listed || !same_root) && other_pattern.may_collide(pattern) {
                return Err(ReleaseError::TagsMayCollide {
                    first: pattern.as_str().to_owned(),
                    second: other_pattern.as_str().to_owned(),
                });
            }
        }
    }
    check_prefixes(configs)
}

fn check_prefixes(configs: &[RootConfig]) -> Result<(), ReleaseError> {
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

pub fn check_release_plz_configs(files: &[(String, String)]) -> Result<(), String> {
    check_unique(&read_all(files)?).map_err(|error| error.to_string())
}

pub fn check_publish_budget(roots: &[RootPlan]) -> Result<(), ReleaseError> {
    let minutes = publish_budget_minutes(roots);
    let limit = TOKEN_LIFETIME_MINUTES - TOKEN_MARGIN_MINUTES;
    if minutes > limit {
        return Err(ReleaseError::PublishBudgetOutlivesToken { minutes, limit });
    }
    Ok(())
}
