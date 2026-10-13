//! Detection, inventory, profile, and task-group coordination.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use velnor_actions_contract::{
    ContractError, RustConfiguration, RustStackConfig, VelnorConfig, task_id_for_stack,
};
use velnor_actions_rust::{
    DeriveInputs, DetectionStatus, FileIndex, Recommendation, RustExecutionProfile, TaskGroup,
    TaskKind, WorkspaceRecord, apply_stack_ignores, check_candidate_outcomes, check_duplicates,
    dedupe_workspaces, derive_task_groups, derive_workspace_fmt, to_detected_projects,
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
}

/// Debug-only consumer-manifest fixture filename under `.velnor`.
#[cfg(debug_assertions)]
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
    /// Tool-input checks: presence, parse, values, digests.
    pub tool_checks: Vec<ToolInputCheck>,
    /// Barrier-separated Clippy memory schedule.
    pub clippy_memory: ClippyMemoryPlan,
    /// Sorted unique recommendations.
    pub recommendations: Vec<String>,
    /// Debug-only release-manifest text; always `None` in release builds.
    pub consumer_manifest_json: Option<String>,
}

/// Run file index, detection, inventory, profiles, and task derivation.
///
/// # Errors
///
/// Returns discovery, detection, inventory, profile, preparation, or
/// contract errors when any stage fails.
pub(crate) fn discover(root: &Path, config: &VelnorConfig) -> Result<Discovery, OrchestratorError> {
    let index = build_file_index(root, &config.discovery.exclude)?;
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
    let workspaces = plan_workspaces(root, &index, &statuses, inventories)?;
    qualify_workspaces(root, &workspaces)?;
    let task_groups = derive_all(config, &index, &workspaces)?;
    let tool_checks = check_tool_inputs(root);
    let clippy_memory = clippy_memory_groups(&task_groups);
    let recommendations = collect_recommendations(&index, &workspaces, &tool_checks);
    Ok(Discovery {
        statuses,
        workspaces,
        task_groups,
        tool_checks,
        clippy_memory,
        recommendations,
        consumer_manifest_json: debug_manifest_fixture(root),
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

/// Debug-only manifest fixture: explicit file, else an embedded
/// `example.invalid` stand-in. Release builds never read the file and
/// have no injection path.
#[cfg(debug_assertions)]
#[expect(clippy::unnecessary_wraps, reason = "release twin returns None")]
fn debug_manifest_fixture(root: &Path) -> Option<String> {
    if let Ok(text) = std::fs::read_to_string(root.join(RELEASE_MANIFEST_REL)) {
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

/// Release builds have no manifest injection path.
#[cfg(not(debug_assertions))]
fn debug_manifest_fixture(_root: &Path) -> Option<String> {
    None
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
        let outcome = profile_for_workspace(root, index, &record)?;
        planned.push(PlannedWorkspace {
            record,
            profile: outcome.profile,
            recommendations: outcome.recommendations,
        });
    }
    planned.sort_by(|left, right| left.record.workspace_root.cmp(&right.record.workspace_root));
    Ok(planned)
}

/// Derive every task group, expanding test shards.
fn derive_all(
    config: &VelnorConfig,
    index: &FileIndex,
    workspaces: &[PlannedWorkspace],
) -> Result<Vec<TaskGroup>, OrchestratorError> {
    let rust = config
        .stacks
        .rust
        .clone()
        .unwrap_or_else(RustStackConfig::default_config);
    let explicit_fmt = index.contains("rustfmt.toml") || index.contains(".rustfmt.toml");
    let mut groups = Vec::new();
    for workspace in workspaces {
        for config_name in &rust.configurations {
            groups.extend(derive_for_config(
                config,
                &workspace.record,
                &workspace.profile,
                config_name,
                explicit_fmt,
            )?);
        }
    }
    for group in &groups {
        velnor_actions_contract::validate_task_id(&group.task_id)?;
    }
    groups.sort_by(|left, right| left.task_id.cmp(&right.task_id));
    Ok(groups)
}

/// Derive task groups for one workspace and configuration.
fn derive_for_config(
    config: &VelnorConfig,
    record: &WorkspaceRecord,
    profile: &RustExecutionProfile,
    rust_config: &RustConfiguration,
    explicit_fmt: bool,
) -> Result<Vec<TaskGroup>, OrchestratorError> {
    let mut groups = Vec::new();
    for package in &record.packages {
        if !package.in_workspace || package.external {
            continue;
        }
        let inputs = DeriveInputs {
            package,
            profile,
            configuration: &rust_config.name,
            features: &rust_config.features,
            target: &rust_config.target,
            explicit_fmt,
        };
        for group in derive_task_groups(&inputs)? {
            groups.extend(expand_shards(config, &group)?);
        }
    }
    let manifest = workspace_manifest(&record.workspace_root);
    let fmt = derive_workspace_fmt(&manifest, profile, &rust_config.name, &rust_config.target)?;
    groups.push(fmt);
    Ok(groups)
}

/// Expand test groups into per-shard groups when sharding exceeds one.
fn expand_shards(
    config: &VelnorConfig,
    group: &TaskGroup,
) -> Result<Vec<TaskGroup>, ContractError> {
    if !matches!(group.kind, TaskKind::Test | TaskKind::Nextest) || group.no_test_targets {
        return Ok(vec![group.clone()]);
    }
    let shards = shard_count(config, group);
    if shards <= 1 {
        return Ok(vec![group.clone()]);
    }
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
    Ok(expanded)
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
