//! Per-configuration task-group derivation with per-crate features.
//!
//! A configuration's feature list is workspace-global, but features are
//! per-crate: each crate applies only the intersection of the requested
//! features with its own declared features from `cargo metadata`. A crate
//! declaring none of the requested features falls back to its own default
//! features (the lone `default` sentinel, emitting no feature flags)
//! instead of emitting flags Cargo would reject. Skipping the whole
//! crate-by-configuration cell would drop Clippy, build, and docs for an
//! otherwise healthy crate, so fallback keeps the cell with a recorded
//! [`FeatureFallback`] rendered in the plan. A requested feature no
//! workspace crate declares is a typo and fails closed, naming the crate.

use std::collections::BTreeSet;

use velnor_actions_contract::{RustConfiguration, VelnorConfig};
use velnor_actions_mise::ArchivePlan;
use velnor_actions_rust::{
    DeriveInputs, RustExecutionProfile, TaskGroup, WorkspaceRecord, derive_task_groups,
    derive_workspace_fmt_if_explicit,
};

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::discover::{PlannedWorkspace, expand_shards, workspace_manifest};

/// One configuration whose applied features differ from the request.
///
/// Recorded on [`crate::Discovery`] and rendered in the plan so the
/// fallback is never silent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureFallback {
    /// Crate the fallback applies to.
    pub package_name: String,
    /// Configuration whose request was narrowed.
    pub configuration: String,
    /// Requested features, sorted.
    pub requested: Vec<String>,
    /// Applied features, sorted (`["default"]` for crate defaults).
    pub applied: Vec<String>,
}

impl FeatureFallback {
    /// True when the crate fell back to its own default features.
    #[must_use]
    pub fn used_defaults(&self) -> bool {
        matches!(self.applied.as_slice(), [only] if only == "default")
    }
}

/// Declared features across every planned workspace package.
///
/// The union spans workspaces so a feature declared in one workspace
/// never reads as a typo while resolving another.
#[must_use]
pub(crate) fn declared_union(workspaces: &[PlannedWorkspace]) -> BTreeSet<String> {
    let mut union = BTreeSet::new();
    for workspace in workspaces {
        for package in &workspace.record.packages {
            if package.in_workspace && !package.external {
                union.extend(package.features.iter().cloned());
            }
        }
    }
    union
}

/// Derive task groups for one workspace and configuration.
///
/// Resolves per-crate features before derivation and returns every
/// fallback alongside; unknown features fail closed naming the crate.
/// The workspace fmt group exists only when no member owns the
/// workspace manifest: a root package's per-package fmt group already
/// carries that (`manifest_key`, `fmt`, configuration) task id, so a
/// workspace group alongside it would duplicate the id. Per-package
/// formatting lives in crate jobs either way, so no scope is lost.
///
/// # Errors
///
/// Returns config errors for unknown features and contract errors
/// when a task id cannot be derived.
pub(crate) fn derive_for_config(
    config: &VelnorConfig,
    record: &WorkspaceRecord,
    profile: &RustExecutionProfile,
    rust_config: &RustConfiguration,
    explicit_fmt: bool,
    archives: &mut ArchivePlan,
    union: &BTreeSet<String>,
) -> Result<(Vec<TaskGroup>, Vec<FeatureFallback>), OrchestratorError> {
    let mut groups = Vec::new();
    let mut fallbacks = Vec::new();
    let manifest = workspace_manifest(&record.workspace_root);
    let mut root_package = false;
    for package in &record.packages {
        if !package.in_workspace || package.external {
            continue;
        }
        root_package = root_package || package.manifest == manifest;
        let (features, fallback) =
            resolve_features(package, &rust_config.features, union, &rust_config.name)?;
        fallbacks.extend(fallback);
        let inputs = DeriveInputs {
            package,
            profile,
            configuration: &rust_config.name,
            features: &features,
            target: &rust_config.target,
            explicit_fmt,
        };
        for group in derive_task_groups(&inputs)? {
            groups.extend(expand_shards(config, &group, profile, archives)?);
        }
    }
    if !root_package
        && let Some(fmt) = derive_workspace_fmt_if_explicit(
            &manifest,
            profile,
            &rust_config.name,
            &rust_config.target,
            explicit_fmt,
        )?
    {
        groups.push(fmt);
    }
    Ok((groups, fallbacks))
}

/// Intersect one configuration request with one crate's declared features.
///
/// The lone `default` sentinel and the empty list pass through untouched:
/// they carry no per-crate names to resolve. Otherwise every requested
/// name must exist in the workspace union (typos fail closed naming the
/// crate), the crate applies the sorted intersection, and an empty
/// intersection falls back to the crate's own default features.
///
/// # Errors
///
/// Returns [`OrchestratorError::Config`] naming crate, configuration,
/// and feature when no workspace crate declares a requested feature.
fn resolve_features(
    package: &velnor_actions_rust::PackageRecord,
    requested: &[String],
    union: &BTreeSet<String>,
    configuration: &str,
) -> Result<(Vec<String>, Option<FeatureFallback>), OrchestratorError> {
    if requested.len() == 1 && requested[0] == "default" {
        return Ok((requested.to_vec(), None));
    }
    if requested.is_empty() {
        return Ok((Vec::new(), None));
    }
    for feature in requested {
        if !union.contains(feature) {
            return Err(OrchestratorError::config(
                CONFIG_REL,
                "stacks.rust.configurations.features",
                format!("unknown_feature:{configuration}:{}:{feature}", package.name),
            ));
        }
    }
    let declared: BTreeSet<&str> = package.features.iter().map(String::as_str).collect();
    let mut applied: Vec<String> = requested
        .iter()
        .filter(|feature| declared.contains(feature.as_str()))
        .cloned()
        .collect();
    applied.sort();
    applied.dedup();
    let mut sorted = requested.to_vec();
    sorted.sort();
    sorted.dedup();
    if applied == sorted {
        return Ok((applied, None));
    }
    if applied.is_empty() {
        applied = vec!["default".to_owned()];
    }
    let fallback = FeatureFallback {
        package_name: package.name.clone(),
        configuration: configuration.to_owned(),
        requested: sorted,
        applied: applied.clone(),
    };
    Ok((applied, Some(fallback)))
}
