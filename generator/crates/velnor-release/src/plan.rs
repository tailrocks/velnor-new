// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{Platform, RepoPath, RustMetadata};

use crate::bundle::{ExtraBin, check_targets};
use crate::check::{CheckPlan, plan_checks};
use crate::error::ReleaseError;
use crate::error_family::FamilyError;
use crate::family::{FamilyPlan, plan_binaries_and_family};
use crate::hazards::{
    cargo_config, nightly_manifest, shadowed_by_extensionless_config, symlink_on_config_path,
    toolchain_path,
};
use crate::lock_version::check_versions;
use crate::members::{
    check_inventory, check_registries, inherited_packages, inherited_tag, owner, settle_publishing,
};
use crate::mise;
use crate::reality::check_reality;
use crate::registry::check_publish_registries_in_text;
use crate::release_plz::{RootConfig, read, root_of};
use crate::tag::{PACKAGE_WILDCARD, TagPattern};
use crate::timeout::{TOKEN_LIFETIME_MINUTES, TOKEN_MARGIN_MINUTES, publish_budget};
use crate::velnor_config::{BinaryFormat, VelnorConfig, parse};

#[derive(Debug, Clone, Default)]
pub struct ReleaseFiles {
    pub release_toml: Option<String>,
    pub release_plz_toml: BTreeMap<String, String>,
    pub mise_tomls: Vec<String>,
    pub cargo_configs: BTreeMap<String, String>,
    pub toolchain_files: BTreeMap<String, String>,
    pub symlinks: BTreeSet<String>,
    pub cargo_manifests: Option<BTreeSet<String>>,
    pub cargo_manifest_texts: BTreeMap<String, String>,
    pub bin_targets: Option<BTreeSet<(String, String, String)>>,
    pub rust_metadata: Option<BTreeMap<RepoPath, RustMetadata>>,
    pub workflow_names: Option<BTreeSet<String>>,
    pub push_workflows: Option<BTreeSet<String>>,
    pub mise_lock: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootPlan {
    pub root: String,
    pub pr_branch_prefix: String,
    pub publishes_to_crates_io: bool,
    pub packages: Vec<(String, TagPattern)>,
    pub inherited: Vec<String>,
    pub publish_budget_nanos: u128,
    pub workspace_tags: Vec<TagPattern>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryPlan {
    pub package: String,
    pub bin: String,
    pub root: String,
    pub platforms: Vec<Platform>,
    pub extra_bins: Vec<ExtraBin>,
    pub include: Vec<String>,
    pub format: BinaryFormat,
    pub attest: bool,
    pub profile: Option<String>,
    pub asset_name: Option<String>,
    pub tag_pattern: TagPattern,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasePlan {
    pub roots: Vec<RootPlan>,
    pub after: Vec<String>,
    pub pr_only_checks: Vec<String>,
    pub binaries: Vec<BinaryPlan>,
    pub checks: Vec<CheckPlan>,
    pub family: Option<FamilyPlan>,
    pub release_plz_version: String,
    pub release_plz_url: String,
    pub release_plz_sha256: String,
}

pub(crate) fn check_unique(configs: &[RootConfig]) -> Result<(), ReleaseError> {
    let mut tags: BTreeMap<&TagPattern, Vec<String>> = BTreeMap::new();
    for config in configs {
        for package in &config.packages {
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
    let mut all: Vec<(&str, &TagPattern, Option<&str>)> = Vec::new();
    for config in configs {
        let root = config.root.as_str();
        all.extend(
            config
                .packages
                .iter()
                .map(|package| (root, &package.tag_pattern, Some(package.name.as_str()))),
        );
        all.extend(
            config
                .workspace_tags
                .iter()
                .filter(|pattern| !pattern.as_str().contains(PACKAGE_WILDCARD))
                .map(|pattern| (root, pattern, None)),
        );
    }
    for (index, (root, pattern, name)) in all.iter().enumerate() {
        for (other_root, other_pattern, other_name) in &all[index + 1..] {
            let same_root = root == other_root;
            if let (Some(name), Some(other_name)) = (name, other_name)
                && name == other_name
            {
                return Err(ReleaseError::DuplicatePackageName {
                    package: (*name).to_owned(),
                    roots: vec![(*root).to_owned(), (*other_root).to_owned()],
                });
            }
            let both_listed = name.is_some() && other_name.is_some();
            if (both_listed || !same_root) && other_pattern.may_collide(pattern) {
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

pub(crate) fn read_all(files: &[(String, String)]) -> Result<Vec<RootConfig>, String> {
    files
        .iter()
        .map(|(file, text)| read(root_of(file), text).map_err(|error| format!("{file}: {error}")))
        .collect()
}

pub fn check_release_plz_configs(files: &[(String, String)]) -> Result<(), String> {
    check_unique(&read_all(files)?).map_err(|error| error.to_string())
}

fn config_reaches(file: &str, root: &str) -> bool {
    let base = file
        .strip_suffix("/.cargo/config.toml")
        .or_else(|| file.strip_suffix("/.cargo/config"))
        .unwrap_or(".");
    reaches(base, root)
}

fn manifest_under(directory: &str, root: &str) -> bool {
    root == "." || directory == root || directory.starts_with(&format!("{root}/"))
}

fn reaches(base: &str, root: &str) -> bool {
    base == "." || root == base || root.starts_with(&format!("{base}/"))
}

pub(crate) fn manifest_of(root: &str) -> String {
    if root == "." {
        "Cargo.toml".to_owned()
    } else {
        format!("{root}/Cargo.toml")
    }
}

pub(crate) fn plan_binaries(
    config: &VelnorConfig,
    configs: &[RootConfig],
    files: &ReleaseFiles,
) -> Result<Vec<BinaryPlan>, ReleaseError> {
    let mut binaries = Vec::new();
    for binary in &config.binaries {
        let (root, tag_pattern) = configs
            .iter()
            .find_map(|candidate| {
                let listed = candidate
                    .packages
                    .iter()
                    .find(|package| package.name == binary.package)
                    .map(|package| package.tag_pattern.clone());
                let inherited = || {
                    let (_, workspace) = owner(&candidate.root, files.rust_metadata.as_ref()?)?;
                    inherited_tag(candidate, workspace, &binary.package)
                };
                listed
                    .or_else(inherited)
                    .map(|pattern| (candidate.root.clone(), pattern))
            })
            .ok_or_else(|| ReleaseError::BinaryNotReleased {
                package: binary.package.clone(),
            })?;
        check_targets(binary, &manifest_of(&root), files)?;
        binaries.push(BinaryPlan {
            package: binary.package.clone(),
            bin: binary.bin.clone(),
            root,
            platforms: binary.platforms.clone(),
            extra_bins: binary.bins.clone(),
            include: binary.include.clone(),
            format: binary.format,
            attest: binary.attest,
            profile: binary.profile.clone(),
            asset_name: crate::asset_name::checked(binary, &tag_pattern)?,
            tag_pattern,
        });
    }
    Ok(binaries)
}

#[must_use]
pub fn publish_budget_minutes(roots: &[RootPlan]) -> u64 {
    let nanos = roots.iter().fold(0u128, |total, root| {
        total.saturating_add(root.publish_budget_nanos)
    });
    crate::timeout::minutes_ceil(nanos)
}

fn plan_roots(
    configs: Vec<RootConfig>,
    metadata: Option<&BTreeMap<RepoPath, RustMetadata>>,
) -> Vec<RootPlan> {
    configs
        .into_iter()
        .map(|config| {
            let inherited = inherited_packages(&config, metadata);
            let budget = publish_budget(&config, &inherited);
            RootPlan {
                inherited,
                publish_budget_nanos: budget,
                root: config.root,
                pr_branch_prefix: config.pr_branch_prefix,
                publishes_to_crates_io: config.publishes_to_crates_io,
                packages: config
                    .packages
                    .into_iter()
                    .map(|package| (package.name, package.tag_pattern))
                    .collect(),
                workspace_tags: config.workspace_tags,
            }
        })
        .collect()
}

pub fn check_publish_budget(roots: &[RootPlan]) -> Result<(), ReleaseError> {
    let minutes = publish_budget_minutes(roots);
    let limit = TOKEN_LIFETIME_MINUTES - TOKEN_MARGIN_MINUTES;
    if minutes > limit {
        return Err(ReleaseError::PublishBudgetOutlivesToken { minutes, limit });
    }
    Ok(())
}

impl ReleasePlan {
    pub fn read(files: &ReleaseFiles) -> Result<ReleasePlan, ReleaseError> {
        let config = parse(files.release_toml.as_deref())?;
        check_reality(&config, files)?;
        let mut configs = Vec::new();
        for root in &config.roots {
            let text = files
                .release_plz_toml
                .get(root)
                .ok_or_else(|| ReleaseError::MissingReleasePlzToml { root: root.clone() })?;
            configs.push(read(root, text)?);
        }
        check_unique(&configs)?;
        check_inventory(&configs, files.rust_metadata.as_ref())?;
        if let Some(metadata) = &files.rust_metadata {
            settle_publishing(&mut configs, metadata);
        }
        check_versions(&configs, files)?;
        let release_plz = mise::check(&files.mise_tomls, &files.mise_lock)?;
        for root in &config.roots {
            symlink_on_config_path(root, &files.symlinks)?;
        }
        match &files.rust_metadata {
            Some(metadata) => check_registries(&configs, metadata, &files.cargo_manifest_texts)?,
            None => {
                for config in &configs {
                    check_publish_registries_in_text(config, &files.cargo_manifest_texts)?;
                }
            }
        }
        for (file, text) in &files.cargo_configs {
            let mut reached = configs
                .iter()
                .filter(|config| config_reaches(file, &config.root))
                .peekable();
            if reached.peek().is_some() {
                cargo_config(
                    file,
                    text,
                    reached.any(|config| config.publishes_to_crates_io),
                    shadowed_by_extensionless_config(file, |other| {
                        files.cargo_configs.contains_key(other)
                    }),
                )?;
            }
        }
        for (file, text) in &files.cargo_manifest_texts {
            let directory = file.strip_suffix("/Cargo.toml").unwrap_or(".");
            if configs.iter().any(|config| {
                reaches(directory, &config.root) || manifest_under(directory, &config.root)
            }) {
                nightly_manifest(file, text)?;
            }
        }
        for (file, text) in files
            .toolchain_files
            .iter()
            .filter(|(file, _)| !file.contains('/'))
        {
            toolchain_path(file, text)?;
        }
        let (family, binaries) = plan_binaries_and_family(&config, &configs, files)?;
        if family.is_some() && !config.checks.is_empty() {
            return Err(FamilyError::Unsupported { key: "[[checks]]" }.into());
        }
        let checks = plan_checks(config.checks, &binaries)?;
        let roots = plan_roots(configs, files.rust_metadata.as_ref());
        Ok(ReleasePlan {
            roots,
            after: config.after,
            pr_only_checks: config.pr_only_checks,
            binaries,
            checks,
            family,
            release_plz_version: release_plz.version,
            release_plz_url: release_plz.url,
            release_plz_sha256: release_plz.sha256,
        })
    }
}
