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

use velnor_actions_contract::{
    ContractError, FileIndex, RustConfiguration, Stack, VelnorConfig, task_id_for_stack,
};
use velnor_actions_mise::{ArchivePlan, NextestArchive, NextestDriver, SortedInventory};
use velnor_actions_rust::{
    CompileDriver, DeriveInputs, RustExecutionProfile, TaskGroup, TaskKind, derive_task_groups,
    derive_workspace_fmt_if_explicit, expand_shards_for_group,
};

use crate::OrchestratorError;
use crate::discover::{PlannedWorkspace, workspace_manifest};

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
pub(crate) fn declared_union(
    workspaces: &[PlannedWorkspace],
    index: &FileIndex,
) -> BTreeSet<String> {
    let mut union = BTreeSet::new();
    for workspace in workspaces {
        for package in &workspace.record.packages {
            if package.in_workspace && !package.external && index.contains(&package.manifest) {
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
///
/// # Errors
///
/// Returns config errors for unknown features and contract errors
/// when a task id cannot be derived.
pub(crate) fn derive_for_config(
    config: &VelnorConfig,
    index: &FileIndex,
    workspace: &PlannedWorkspace,
    rust_config: &RustConfiguration,
    explicit_fmt: bool,
    archives: &mut ArchivePlan,
    union: &BTreeSet<String>,
) -> Result<(Vec<TaskGroup>, Vec<FeatureFallback>), OrchestratorError> {
    let record = &workspace.record;
    let profile = &workspace.profile;
    let mut groups = Vec::new();
    let mut fallbacks = Vec::new();
    for package in &record.packages {
        if !package.in_workspace || package.external || !index.contains(&package.manifest) {
            continue;
        }
        let (features, fallback) = crate::rust_features::resolve(package, rust_config, union)?;
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
    // Per-package Fmt groups already cover every file: a workspace Fmt group
    // for the same config would re-check them via plan `fmt --all` (R28).
    // Package-less workspaces derive no per-package groups, so their one
    // distinct workspace scope still lands below. Suppression also owns the
    // root-manifest-once invariant: with no second group there is no second
    // `stack/rust/<key>/fmt/<config>` id, so plan-v1 can never trip
    // `duplicate_task_id` here. A manifest-ownership partition (emit unless
    // a member owns the root manifest) agrees in every case except virtual
    // workspaces with members, where it would re-add the overlapping
    // whole-workspace check P05-5 forbids; per-package presence is the rule.
    let per_package_fmt = groups
        .iter()
        .any(|group| group.kind == TaskKind::Fmt && !group.package_name.is_empty());
    if !per_package_fmt
        && let Some(fmt) = derive_workspace_fmt_if_explicit(
            &workspace_manifest(&record.workspace_root),
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

/// Expand test groups into per-shard groups when sharding exceeds one.
pub(crate) fn expand_shards(
    config: &VelnorConfig,
    group: &TaskGroup,
    profile: &RustExecutionProfile,
    archives: &mut ArchivePlan,
) -> Result<Vec<TaskGroup>, ContractError> {
    let shards = shard_count(config, group);
    if !expand_shards_for_group(group.kind, group.test_runner, shards, group.no_test_targets)? {
        return Ok(vec![group.clone()]);
    }
    plan_shard_archive(group, profile, archives)?;
    let mut expanded = Vec::new();
    for shard in 1..=shards {
        let task_id = task_id_for_stack(
            Stack::Rust.id(),
            &group.manifest_key,
            group.kind.as_str(),
            &group.configuration,
            Some((shard, shards)),
        )?;
        let mut sharded = group.clone();
        sharded.task_id = task_id;
        expanded.push(sharded);
    }
    let mut ids: Vec<String> = expanded.iter().map(|group| group.task_id.clone()).collect();
    ids.sort();
    SortedInventory::from_sorted(ids)
        .map_err(|err| ContractError::identity("shard_inventory", err.to_string()))?;
    Ok(expanded)
}

/// Record one archive per package/config under the resolved profile.
///
/// # Errors
///
/// Returns a contract error for an unplannable archive (X9: never
/// silently skip archive planning). The compile driver is a typed enum,
/// so an unknown driver is unrepresentable.
fn plan_shard_archive(
    group: &TaskGroup,
    profile: &RustExecutionProfile,
    archives: &mut ArchivePlan,
) -> Result<(), ContractError> {
    let driver = match group.compile_driver {
        CompileDriver::Cargo => NextestDriver::Cargo,
        CompileDriver::Mbx => NextestDriver::Mbx,
    };
    let target = if group.target == "host" {
        None
    } else {
        Some(group.target.as_str())
    };
    let archive = NextestArchive::with_profile(
        driver,
        &group.package_name,
        &group.features,
        target,
        profile.nextest_profile.as_str(),
    )
    .map_err(|err| ContractError::identity("archive_plan", err.to_string()))?;
    if archives.add(&archive).is_err() {
        // Archive already planned for this package/config.
    }
    Ok(())
}

/// Shard count for one group from the sharding policy.
fn shard_count(config: &VelnorConfig, group: &TaskGroup) -> u32 {
    let manifest = manifest_for_key(&group.manifest_key);
    let shards = &config.test_sharding;
    shards
        .by_manifest
        .get(&manifest)
        .copied()
        .unwrap_or(shards.default_shards)
}

/// Manifest path for a manifest key.
fn manifest_for_key(key: &str) -> String {
    if key == "root" {
        "Cargo.toml".to_owned()
    } else {
        format!("{key}/Cargo.toml")
    }
}

#[cfg(test)]
mod tests {
    use velnor_actions_mise::ArchivePlan;
    use velnor_actions_rust::{
        CompileDriver, NextestProfile, ProfileSource, RustExecutionProfile, TaskGroup, TaskKind,
        TestRunner,
    };

    use super::plan_shard_archive;

    /// Minimal group with driver, target, and package selectors.
    fn group(driver: CompileDriver, target: &str) -> TaskGroup {
        TaskGroup {
            task_id: "stack/rust/root/nextest/default".to_owned(),
            package_id: "demo".to_owned(),
            package_name: "demo".to_owned(),
            manifest_key: "root".to_owned(),
            kind: TaskKind::Nextest,
            configuration: "default".to_owned(),
            features: Vec::new(),
            target: target.to_owned(),
            gated_by: Vec::new(),
            depends_on: Vec::new(),
            target_flags: Vec::new(),
            no_test_targets: false,
            package_arg: None,
            compile_driver: driver,
            test_runner: TestRunner::CargoNextest,
            declared_inputs: Vec::new(),
            undeclared_reads: false,
            uses_network: false,
            uses_clock: false,
            uses_random: false,
            nextest_profile: NextestProfile::Default,
        }
    }

    /// Detected Cargo/Nextest profile selecting the default Nextest profile.
    fn profile() -> RustExecutionProfile {
        RustExecutionProfile {
            compile_driver: CompileDriver::Cargo,
            test_runner: TestRunner::CargoNextest,
            evidence: Vec::new(),
            driver_source: ProfileSource::Detected,
            runner_source: ProfileSource::Detected,
            nextest_profile: NextestProfile::Default,
            nextest_config: None,
        }
    }

    #[test]
    fn archive_planning_errors_instead_of_skipping() {
        let profile = profile();
        let mut archives = ArchivePlan::new();
        let err = plan_shard_archive(
            &group(CompileDriver::Cargo, "${{ x }}"),
            &profile,
            &mut archives,
        )
        .expect_err("bad target fails");
        assert!(err.to_string().contains("archive_plan"), "{err}");
        assert!(archives.is_empty());
        plan_shard_archive(
            &group(CompileDriver::Cargo, "host"),
            &profile,
            &mut archives,
        )
        .expect("planned");
        assert_eq!(archives.len(), 1);
        // Re-planning the same configuration dedupes without error.
        plan_shard_archive(
            &group(CompileDriver::Cargo, "host"),
            &profile,
            &mut archives,
        )
        .expect("dedupe");
        assert_eq!(archives.len(), 1);
    }
}
