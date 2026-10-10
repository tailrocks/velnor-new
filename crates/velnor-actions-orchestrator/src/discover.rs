//! Detection, inventory, profile, and task-proposal coordination.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use velnor_actions_contract::{
    DETECTION_SCHEMA, DetectionStatus, FileIndex, ProposedTask, RustStackConfig, VelnorConfig,
    WorkflowTask, apply_stack_ignores, check_candidate_outcomes, check_duplicates,
    selected_projects,
};
#[path = "discovery_registry.rs"]
mod registry;
pub(crate) use registry::detector_entries;
use registry::{DETECTORS, detected_projects};

use velnor_actions_mise::ArchivePlan;
use velnor_actions_rust::{
    Recommendation, RustExecutionProfile, WorkspaceRecord, dedupe_workspaces, propose_task,
};

use crate::OrchestratorError;
use crate::clippy_groups::{ClippyMemoryPlan, clippy_memory_groups};
use crate::discover_index::build_file_index;
use crate::internal::phase_timing::PlanPhaseTimings;
use crate::inventory::{qualify_workspaces, run_inventories, run_inventories_with_phase_timings};
use crate::recommendations::collect_recommendations;
#[path = "consumer_manifest.rs"]
mod consumer_manifest;
use crate::toolcheck::{ToolInputCheck, check_tool_inputs_with_paths};
use crate::{discover_tofu::qualify_tofu_step, evidence::profile_for_workspace};

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

/// Full detection output feeding planning and rendering.
#[derive(Debug, Clone)]
pub struct Discovery {
    /// Explicit repository-owned checks, independent of detected workspaces.
    pub mise_checks: Vec<velnor_actions_mise::DiscoveredCheck>,
    /// Per-project selection states.
    pub statuses: Vec<DetectionStatus>,
    /// Selected workspaces with profiles.
    pub workspaces: Vec<PlannedWorkspace>,
    /// Validated adapter task proposals sorted by task ID.
    pub proposals: Vec<ProposedTask>,
    /// Per-crate feature fallbacks in derivation order.
    pub feature_fallbacks: Vec<crate::derive_groups::FeatureFallback>,
    /// Tool-input checks: presence, parse, values, digests.
    pub tool_checks: Vec<ToolInputCheck>,
    /// Barrier-separated Clippy memory schedule.
    pub clippy_memory: ClippyMemoryPlan,
    /// Non-fatal generation recommendations.
    pub recommendations: Vec<String>,
    /// Release-manifest text for consumer policy from the committed file.
    ///
    /// Consumer debug builds fall back to a stand-in when the file is absent
    /// (flagged by [`Discovery::consumer_manifest_stand_in`], warned at
    /// generation); release builds keep `None` so generation fails
    /// closed with `consumer_requires_release_install`.
    pub consumer_manifest_json: Option<String>,
    /// Whether the manifest text above is the debug-only stand-in.
    ///
    /// False for Velnor policy and release builds. `generate` warns when
    /// consumer policy uses the debug stand-in; `plan` stays silent.
    pub consumer_manifest_stand_in: bool,
    /// Whether non-UTF-8 names require broad selection.
    pub skipped_non_utf8: bool,
    /// Tofu plan note: ignore marker or table-less evidence advisory.
    ///
    /// `None` when the tofu table is absent and no evidence exists, or
    /// when configured roots convert to selected projects.
    pub tofu_note: Option<velnor_actions_tofu::TofuNote>,
    /// Tofu selection records: head files plus edges per root.
    pub tofu_units: Vec<crate::select_tofu::TofuSelectionUnit>,
}

/// Run file index, detection, inventory, profiles, and task derivation.
///
/// # Errors
///
/// Returns discovery, detection, inventory, profile, preparation, or
/// contract errors when any stage fails.
pub(crate) fn discover(root: &Path, config: &VelnorConfig) -> Result<Discovery, OrchestratorError> {
    discover_inner(root, config, None)
}

pub(crate) fn discover_with_phase_timings(
    root: &Path,
    config: &VelnorConfig,
    phases: &mut PlanPhaseTimings,
) -> Result<Discovery, OrchestratorError> {
    discover_inner(root, config, Some(phases))
}

fn discover_inner(
    root: &Path,
    config: &VelnorConfig,
    mut phases: Option<&mut PlanPhaseTimings>,
) -> Result<Discovery, OrchestratorError> {
    let (index, skipped_non_utf8) = build_file_index(root, &config.discovery.exclude)?;
    let mut candidates = Vec::new();
    let mut previous = "";
    for (stack_id, schema, detect) in DETECTORS {
        debug_assert!(previous < stack_id, "registry runs ascending");
        debug_assert_eq!(schema, DETECTION_SCHEMA, "registry schema");
        previous = stack_id;
        candidates.extend(detect(&index));
    }
    let task_tool_paths = config
        .workflow
        .tasks
        .iter()
        .filter_map(|task| match task {
            WorkflowTask::Verification(task) => Some(&task.source),
            WorkflowTask::Build(_) | WorkflowTask::NativeImage(_) => None,
        })
        .flat_map(|source| {
            [
                source.mise_config.clone(),
                source.mise_lock_path(),
                source.rust_toolchain_path(),
            ]
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let tool_checks = check_tool_inputs_with_paths(root, &task_tool_paths);
    let mut reads = velnor_actions_tofu::FileCache::new();
    let tofu_step = qualify_tofu_step(root, config, &index, &tool_checks, &mut reads)?;
    candidates.extend(tofu_step.candidates);
    let projects = detected_projects(&candidates)?;
    check_duplicates(&projects).map_err(|err| OrchestratorError::Detection {
        problem: err.to_string(),
    })?;
    let initial = apply_stack_ignores(projects, &config.stacks.ignore);
    #[expect(
        clippy::needless_option_as_deref,
        reason = "the original Option must remain available after this branch"
    )]
    let inventory_result = match phases.as_deref_mut() {
        Some(phases) => {
            run_inventories_with_phase_timings(root, &candidates, index.files(), &mut reads, phases)
        }
        None => run_inventories(root, &candidates, index.files(), &mut reads),
    };
    let ((outcomes, inventories), tofu_units) = inventory_result?;
    let statuses = check_candidate_outcomes(initial, &outcomes).map_err(|err| {
        OrchestratorError::Detection {
            problem: err.to_string(),
        }
    })?;
    let workspaces = plan_workspaces(root, &index, &statuses, inventories, config)?;
    qualify_workspaces(root, &workspaces)?;
    let (mut proposals, fallbacks) = derive_all(config, &index, &workspaces, &statuses)?;
    let mise_checks =
        velnor_actions_mise::discover_checks(root, &config.checks, &config.qualified_tools)
            .map_err(|err| OrchestratorError::Contract {
                problem: err.to_string(),
            })?;
    proposals.extend(mise_checks.iter().map(|item| item.proposal.clone()));
    proposals.sort_by(|a, b| a.task_id.cmp(&b.task_id));
    let clippy_memory = clippy_memory_groups(&proposals);
    let recommendations =
        collect_recommendations(root, config, &index, &workspaces, &tool_checks, &mut reads);
    let (consumer_manifest_json, consumer_manifest_stand_in) =
        consumer_manifest::for_policy(root, config.workflow.policy)?;
    Ok(Discovery {
        mise_checks,
        statuses,
        workspaces,
        proposals,
        feature_fallbacks: fallbacks,
        tool_checks,
        clippy_memory,
        recommendations,
        consumer_manifest_json,
        consumer_manifest_stand_in,
        skipped_non_utf8,
        tofu_note: tofu_step.note,
        tofu_units,
    })
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
    let selected = selected_projects(statuses);
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

/// Derive every task proposal (rust groups plus tofu triples) and fallbacks.
fn derive_all(
    config: &VelnorConfig,
    index: &FileIndex,
    workspaces: &[PlannedWorkspace],
    statuses: &[DetectionStatus],
) -> Result<
    (
        Vec<ProposedTask>,
        Vec<crate::derive_groups::FeatureFallback>,
    ),
    OrchestratorError,
> {
    let rust = config
        .stacks
        .rust
        .clone()
        .unwrap_or_else(RustStackConfig::default_config);
    let explicit_fmt = index.contains("rustfmt.toml") || index.contains(".rustfmt.toml");
    let union = crate::derive_groups::declared_union(workspaces, index, config.workflow.policy);
    let mut groups = Vec::new();
    let mut fallbacks = Vec::new();
    let mut archives = ArchivePlan::new();
    for workspace in workspaces {
        for config_name in &rust.configurations {
            let (derived, narrowed) = crate::derive_groups::derive_for_config(
                config,
                index,
                workspace,
                config_name,
                explicit_fmt,
                &mut archives,
                &union,
            )?;
            groups.extend(derived);
            fallbacks.extend(narrowed);
        }
    }
    let mut proposals = Vec::with_capacity(groups.len());
    for group in &groups {
        let task = propose_task(group)?;
        task.validate()?;
        proposals.push(task);
    }
    proposals.extend(crate::select_tofu::derive_tofu(statuses, index.files())?);
    proposals.sort_by(|left, right| left.task_id.cmp(&right.task_id));
    Ok((proposals, fallbacks))
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
