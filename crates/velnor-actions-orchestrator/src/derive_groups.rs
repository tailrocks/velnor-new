use std::collections::BTreeSet;

use velnor_actions_contract::{
    ContractError, FileIndex, RustConfiguration, Stack, VelnorConfig, WorkflowPolicy,
    task_id_for_stack,
};
use velnor_actions_mise::{ArchivePlan, NextestArchive, NextestDriver, SortedInventory};
use velnor_actions_rust::{
    CompileDriver, DeriveInputs, RustExecutionProfile, TaskGroup, TaskKind, VelnorV1TaskOwner,
    derive_task_groups, derive_workspace_fmt_if_explicit, expand_shards_for_group,
};

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::discover::{PlannedWorkspace, workspace_manifest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureFallback {
    pub package_name: String,
    pub configuration: String,
    pub requested: Vec<String>,
    pub applied: Vec<String>,
}

impl FeatureFallback {
    #[must_use]
    pub fn used_defaults(&self) -> bool {
        matches!(self.applied.as_slice(), [only] if only == "default")
    }
}

#[must_use]
pub(crate) fn declared_union(
    workspaces: &[PlannedWorkspace],
    index: &FileIndex,
    policy: WorkflowPolicy,
) -> BTreeSet<String> {
    let mut union = BTreeSet::new();
    for workspace in workspaces {
        for package in &workspace.record.packages {
            if package.in_workspace
                && !package.external
                && index.contains(&package.manifest)
                && !repository_maintenance_member(policy, &workspace.record.workspace_root, package)
            {
                union.extend(package.features.iter().cloned());
            }
        }
    }
    union
}

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
    let clippy_all_features = config
        .stacks
        .rust
        .as_ref()
        .is_some_and(|rust| rust.clippy_all_features);
    let mut groups = Vec::new();
    let mut fallbacks = Vec::new();
    for package in &record.packages {
        if !package.in_workspace
            || package.external
            || !index.contains(&package.manifest)
            || repository_maintenance_member(
                config.workflow.policy,
                &record.workspace_root,
                package,
            )
        {
            continue;
        }
        let (features, fallback) = if rust_config.all_features {
            (package.features.clone(), None)
        } else {
            resolve_features(package, &rust_config.features, union, &rust_config.name)?
        };
        fallbacks.extend(fallback);
        let inputs = DeriveInputs {
            package,
            profile,
            configuration: &rust_config.name,
            features: &features,
            all_features: rust_config.all_features,
            clippy_all_features,
            target: &rust_config.target,
            explicit_fmt,
        };
        for group in derive_task_groups(&inputs)? {
            groups.extend(expand_shards(config, &group, profile, archives)?);
        }
    }
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

fn repository_maintenance_member(
    policy: WorkflowPolicy,
    workspace_root: &str,
    package: &velnor_actions_rust::PackageRecord,
) -> bool {
    policy == WorkflowPolicy::VelnorRepositoryV1
        && workspace_root.is_empty()
        && package.in_workspace
        && !package.external
        && package.v1_task_owner == VelnorV1TaskOwner::RepositoryMaintenance
}

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
    let _duplicate = archives.add(&archive).is_err();
    Ok(())
}

fn shard_count(config: &VelnorConfig, group: &TaskGroup) -> u32 {
    let manifest = manifest_for_key(&group.manifest_key);
    let shards = &config.test_sharding;
    shards
        .by_manifest
        .get(&manifest)
        .copied()
        .unwrap_or(shards.default_shards)
}

fn manifest_for_key(key: &str) -> String {
    if key == "root" {
        "Cargo.toml".to_owned()
    } else {
        format!("{key}/Cargo.toml")
    }
}
