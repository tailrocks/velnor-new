//! Detection, inventory, profile, and task-group coordination.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use velnor_actions_contract::{ContractError, RustStackConfig, VelnorConfig, task_id_for_stack};
use velnor_actions_mise::{ArchivePlan, NextestArchive, NextestDriver, SortedInventory};
use velnor_actions_rust::{
    DetectionStatus, FileIndex, Recommendation, RustExecutionProfile, TaskGroup, WorkspaceRecord,
    apply_stack_ignores, check_candidate_outcomes, check_duplicates, dedupe_workspaces,
    expand_shards_for_group, to_detected_projects,
};

use crate::OrchestratorError;
use crate::clippy_groups::{ClippyMemoryPlan, clippy_memory_groups};
use crate::discover_index::build_file_index;
use crate::evidence::profile_for_workspace;
use crate::inventory::{qualify_workspaces, run_inventories};
use crate::recommendations::collect_recommendations;
use crate::toolcheck::{ToolInputCheck, check_tool_inputs};

/// One workspace with its inventory, profile, and recommendations.
#[derive(Debug, Clone)]
pub struct PlannedWorkspace {
    /// Parsed Cargo inventory.
    pub record: WorkspaceRecord,
    /// Selected execution profile.
    pub profile: RustExecutionProfile,
    /// Profile recommendations.
    pub recommendations: Vec<Recommendation>,
    /// Blocking profile findings; `generate` fails closed when non-empty.
    pub findings: Vec<velnor_actions_rust::ProfileFinding>,
}

/// Committed consumer-manifest filename under `.velnor`.
const RELEASE_MANIFEST_REL: &str = ".velnor/release-manifest.json";

/// Full detection output feeding planning and rendering.
#[derive(Debug, Clone)]
pub struct Discovery {
    /// Per-project selection states.
    pub statuses: Vec<DetectionStatus>,
    /// Selected workspaces with profiles.
    pub workspaces: Vec<PlannedWorkspace>,
    /// Derived task groups sorted by task ID.
    pub task_groups: Vec<TaskGroup>,
    /// Per-crate feature fallbacks in derivation order.
    pub feature_fallbacks: Vec<crate::derive_groups::FeatureFallback>,
    /// Tool-input checks: presence, parse, values, digests.
    pub tool_checks: Vec<ToolInputCheck>,
    /// Barrier-separated Clippy memory schedule.
    pub clippy_memory: ClippyMemoryPlan,
    /// Sorted unique recommendations.
    pub recommendations: Vec<String>,
    /// Release-manifest text from the committed repo file.
    ///
    /// Debug builds fall back to an `example.invalid` stand-in when the
    /// file is absent; release builds keep `None` so generation fails
    /// closed with `consumer_requires_release_install`.
    pub consumer_manifest_json: Option<String>,
    /// Whether index enumeration skipped any non-UTF-8 name.
    ///
    /// Selection broadens explicitly on this: a skipped name cannot be
    /// attributed to an owning package.
    pub skipped_non_utf8: bool,
}

/// Run file index, detection, inventory, profiles, and task derivation.
///
/// # Errors
///
/// Returns discovery, detection, inventory, profile, preparation, or
/// contract errors when any stage fails.
pub(crate) fn discover(root: &Path, config: &VelnorConfig) -> Result<Discovery, OrchestratorError> {
    let (index, skipped_non_utf8) = build_file_index(root, &config.discovery.exclude)?;
    let mut candidates = Vec::new();
    let mut previous = "";
    for (stack_id, schema, detect) in DETECTORS {
        debug_assert!(previous < stack_id, "registry runs ascending");
        debug_assert_eq!(schema, DETECTION_SCHEMA, "registry schema");
        previous = stack_id;
        candidates.extend(detect(&index));
    }
    let projects = to_detected_projects(&candidates);
    check_duplicates(&projects).map_err(|err| OrchestratorError::Detection {
        problem: err.to_string(),
    })?;
    let initial = apply_stack_ignores(projects, &config.stacks.ignore);
    let (outcomes, inventories) = run_inventories(root, &candidates)?;
    let statuses = check_candidate_outcomes(initial, &outcomes).map_err(|err| {
        OrchestratorError::Detection {
            problem: err.to_string(),
        }
    })?;
    let workspaces = plan_workspaces(root, &index, &statuses, inventories, config)?;
    qualify_workspaces(root, &workspaces)?;
    let (task_groups, fallbacks) = derive_all(config, &index, &workspaces)?;
    let tool_checks = check_tool_inputs(root);
    let clippy_memory = clippy_memory_groups(&task_groups);
    let recommendations = collect_recommendations(&index, &workspaces, &tool_checks);
    Ok(Discovery {
        statuses,
        workspaces,
        task_groups,
        feature_fallbacks: fallbacks,
        tool_checks,
        clippy_memory,
        recommendations,
        consumer_manifest_json: consumer_manifest_text(root),
        skipped_non_utf8,
    })
}

/// Detector registry (stack id, record schema, implementation), ascending.
/// V1 registers rust; schema 1 is `{ stack_id, project_root, manifest }`.
type DetectorEntry = (
    &'static str,
    u32,
    fn(&FileIndex) -> Vec<velnor_actions_rust::CargoCandidate>,
);
const DETECTION_SCHEMA: u32 = 1;
const DETECTORS: [DetectorEntry; 1] = [(
    velnor_actions_rust::STACK_ID,
    DETECTION_SCHEMA,
    velnor_actions_rust::discover_candidates,
)];

/// Registered detectors as (stack ID, record schema), ascending.
pub(crate) fn detector_entries() -> Vec<(&'static str, u32)> {
    DETECTORS
        .iter()
        .map(|(stack_id, schema, _)| (*stack_id, *schema))
        .collect()
}

/// Read the committed release-manifest file; absent/unreadable is `None`.
///
/// Cfg-independent so tests (debug assertions on) cover the exact read
/// the release twin relies on; schema and version validation happen
/// downstream in the consumer acquire gate.
pub(crate) fn read_manifest_file(root: &Path) -> Option<String> {
    std::fs::read_to_string(root.join(RELEASE_MANIFEST_REL)).ok()
}

/// Consumer manifest text: committed file, else an embedded
/// `example.invalid` stand-in (debug builds only).
#[cfg(debug_assertions)]
#[expect(clippy::unnecessary_wraps, reason = "release twin returns None")]
fn consumer_manifest_text(root: &Path) -> Option<String> {
    if let Some(text) = read_manifest_file(root) {
        return Some(text);
    }
    let sha = "a".repeat(64);
    let mut targets = Vec::new();
    for target in [
        "x86_64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
    ] {
        targets.push(format!(
            "{{\"target\":\"{target}\",\"artifact\":\"https://example.invalid/r/{target}\",\"sha256\":\"{sha}\"}}"
        ));
    }
    let targets = targets.join(",");
    let version = env!("CARGO_PKG_VERSION");
    Some(format!(
        "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"targets\":[{targets}]}}"
    ))
}

/// Consumer manifest text: the committed file, with no stand-in.
///
/// Absent or unreadable files stay `None`; the consumer acquire gate
/// fails closed with `consumer_requires_release_install`.
#[cfg(not(debug_assertions))]
fn consumer_manifest_text(root: &Path) -> Option<String> {
    read_manifest_file(root)
}

/// Sorted local dependency display names for one package.
#[must_use]
pub(crate) fn local_dep_names(record: &WorkspaceRecord, package_id: &str) -> Vec<String> {
    let names: BTreeMap<&str, &str> = record
        .packages
        .iter()
        .map(|pkg| (pkg.id.as_str(), pkg.name.as_str()))
        .collect();
    let mut deps: BTreeSet<&str> = BTreeSet::new();
    for edge in &record.edges {
        if edge.from == package_id
            && let Some(name) = names.get(edge.to.as_str())
        {
            deps.insert(name);
        }
    }
    deps.into_iter().map(str::to_owned).collect()
}

/// Keep selected workspaces, detect profiles, fail incomplete tooling.
fn plan_workspaces(
    root: &Path,
    index: &FileIndex,
    statuses: &[DetectionStatus],
    inventories: Vec<(String, WorkspaceRecord)>,
    config: &VelnorConfig,
) -> Result<Vec<PlannedWorkspace>, OrchestratorError> {
    let selected = velnor_actions_rust::selected_projects(statuses);
    let selected_manifests: BTreeSet<&str> = selected
        .iter()
        .map(|project| project.manifest.as_str())
        .collect();
    let keep_roots: BTreeSet<_> = inventories
        .iter()
        .filter(|(manifest, _)| selected_manifests.contains(manifest.as_str()))
        .map(|(_, record)| record.workspace_root.clone())
        .collect();
    let records: Vec<WorkspaceRecord> = inventories.into_iter().map(|(_, record)| record).collect();
    let mut planned = Vec::new();
    for record in dedupe_workspaces(records) {
        if !keep_roots.contains(&record.workspace_root) {
            continue;
        }
        let outcome = profile_for_workspace(root, index, &record, config.stacks.rust.as_ref())?;
        planned.push(PlannedWorkspace {
            record,
            profile: outcome.profile,
            recommendations: outcome.recommendations,
            findings: outcome.findings,
        });
    }
    planned.sort_by(|left, right| left.record.workspace_root.cmp(&right.record.workspace_root));
    Ok(planned)
}

/// Derive every task group plus feature fallbacks, expanding test shards.
fn derive_all(
    config: &VelnorConfig,
    index: &FileIndex,
    workspaces: &[PlannedWorkspace],
) -> Result<(Vec<TaskGroup>, Vec<crate::derive_groups::FeatureFallback>), OrchestratorError> {
    let rust = config
        .stacks
        .rust
        .clone()
        .unwrap_or_else(RustStackConfig::default_config);
    let explicit_fmt = index.contains("rustfmt.toml") || index.contains(".rustfmt.toml");
    let union = crate::derive_groups::declared_union(workspaces);
    let mut groups = Vec::new();
    let mut fallbacks = Vec::new();
    let mut archives = ArchivePlan::new();
    for workspace in workspaces {
        for config_name in &rust.configurations {
            let (derived, narrowed) = crate::derive_groups::derive_for_config(
                config,
                &workspace.record,
                &workspace.profile,
                config_name,
                explicit_fmt,
                &mut archives,
                &union,
            )?;
            groups.extend(derived);
            fallbacks.extend(narrowed);
        }
    }
    for group in &groups {
        velnor_actions_contract::validate_task_id(&group.task_id)?;
    }
    groups.sort_by(|left, right| left.task_id.cmp(&right.task_id));
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
    if !expand_shards_for_group(
        group.kind,
        &group.test_runner,
        shards,
        group.no_test_targets,
    )? {
        return Ok(vec![group.clone()]);
    }
    plan_shard_archive(group, profile, archives);
    let mut expanded = Vec::new();
    for shard in 1..=shards {
        let task_id = task_id_for_stack(
            velnor_actions_rust::STACK_ID,
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
fn plan_shard_archive(
    group: &TaskGroup,
    profile: &RustExecutionProfile,
    archives: &mut ArchivePlan,
) {
    let driver = match group.compile_driver.as_str() {
        "cargo" => NextestDriver::Cargo,
        "mbx" => NextestDriver::Mbx,
        _ => return,
    };
    let target = if group.target == "host" {
        None
    } else {
        Some(group.target.as_str())
    };
    let Ok(archive) = NextestArchive::with_profile(
        driver,
        &group.package_name,
        &group.features,
        target,
        profile.nextest_profile.as_str(),
    ) else {
        return;
    };
    if archives.add(&archive).is_err() {
        // Archive already planned for this package/config.
    }
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

/// Workspace-root manifest path for a workspace root.
pub(crate) fn workspace_manifest(workspace_root: &str) -> String {
    if workspace_root.is_empty() {
        "Cargo.toml".to_owned()
    } else {
        format!("{workspace_root}/Cargo.toml")
    }
}

/// Workspace-root lockfile path for a workspace root.
///
/// Shared by qualification and source-prep gating so both agree on
/// which workspaces are pinned.
pub(crate) fn workspace_lock(workspace_root: &str) -> String {
    if workspace_root.is_empty() {
        "Cargo.lock".to_owned()
    } else {
        format!("{workspace_root}/Cargo.lock")
    }
}

#[cfg(test)]
#[path = "discover_manifest_tests.rs"]
mod tests;
