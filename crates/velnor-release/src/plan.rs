// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{Platform, RepoPath, RustMetadata};

use crate::bundle::ExtraBin;
use crate::check::{CheckPlan, plan_checks};
use crate::error::ReleaseError;
use crate::family::{FamilyPlan, check_trigger, plan_binaries_and_family};
use crate::hazards::{
    cargo_config, nightly_manifest, shadowed_by_extensionless_config, symlink_on_config_path,
    toolchain_path,
};
use crate::lock_version::check_versions;
use crate::members::{check_inventory, check_registries, inherited_packages, settle_publishing};
use crate::mise;
use crate::reality::check_reality;
use crate::registry::check_publish_registries_in_text;
use crate::release_plz::{RootConfig, read, root_of};
use crate::shipped::check_shipped;
use crate::tag::TagPattern;
use crate::timeout::publish_budget;
use crate::velnor_config::{BinaryFormat, Checksum, Libc, parse};

mod binaries;
mod checks;

pub(crate) use binaries::plan_binaries;
pub(crate) use checks::check_unique;
pub use checks::{check_publish_budget, check_release_plz_configs};

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
    pub tracked_paths: Option<BTreeSet<String>>,
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
    pub version_groups: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryPlan {
    pub package: String,
    pub bin: String,
    pub root: String,
    pub platforms: Vec<Platform>,
    /// The platforms in `platforms` whose build and files are optional: a failed build never fails
    /// the release, and the release holds a file of one only when its build produced it.
    pub optional_platforms: Vec<Platform>,
    pub extra_bins: Vec<ExtraBin>,
    pub include: Vec<String>,
    pub format: BinaryFormat,
    pub libc: Libc,
    pub attest: bool,
    pub smoke: bool,
    pub commit_file: bool,
    pub checksum_files: Option<Checksum>,
    pub member_digests: Option<Checksum>,
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
    /// The repository declares MBX as its cargo wrapper, so every release
    /// job that runs cargo sets up the mise tools first.
    pub mbx: bool,
}

pub(crate) fn read_all(files: &[(String, String)]) -> Result<Vec<RootConfig>, String> {
    files
        .iter()
        .map(|(file, text)| read(root_of(file), text).map_err(|error| format!("{file}: {error}")))
        .collect()
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
                version_groups: config
                    .packages
                    .iter()
                    .filter_map(|package| Some((package.name.clone(), package.group.clone()?)))
                    .collect(),
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
        if let Some(metadata) = &files.rust_metadata {
            check_shipped(&config.binaries, &configs, metadata)?;
        }
        let checks = plan_checks(config.checks, &binaries)?;
        let roots = plan_roots(configs, files.rust_metadata.as_ref());
        check_trigger(family.as_ref(), &roots)?;
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
            mbx: mise::declares_mbx(&files.mise_tomls),
        })
    }
}
