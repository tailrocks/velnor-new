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

use velnor_actions_mise::{ArchivePlan, ToolCatalog};
use velnor_actions_rust::{
    Recommendation, RustExecutionProfile, WorkspaceRecord, dedupe_workspaces, propose_task,
};

use crate::OrchestratorError;
use crate::clippy_groups::{ClippyMemoryPlan, clippy_memory_groups};
use crate::config::gradle_jobs;
use crate::discover_index::build_file_index;
use crate::internal::phase_timing::PlanPhaseTimings;
use crate::inventory::{qualify_workspaces, run_inventories, run_inventories_with_phase_timings};
use crate::recommendations::collect_recommendations;
#[path = "consumer_manifest.rs"]
mod consumer_manifest;
use crate::toolcheck::{ToolInputCheck, check_tool_inputs_with_paths};
use crate::{discover_tofu::qualify_tofu_step, evidence::profile_for_workspace};

#[derive(Debug, Clone)]
pub struct PlannedWorkspace {
    pub record: WorkspaceRecord,
    pub profile: RustExecutionProfile,
    pub recommendations: Vec<Recommendation>,
    pub findings: Vec<velnor_actions_rust::ProfileFinding>,
}

#[derive(Debug, Clone, Default)]
pub struct Discovery {
    pub(crate) gradle_roots: Vec<crate::config::gradle_jobs::GradleRoot>,
    pub mise_checks: Vec<velnor_actions_mise::DiscoveredCheck>,
    pub native_checks: Vec<velnor_actions_contract::config::NativeHomebrewCheck>,
    pub statuses: Vec<DetectionStatus>,
    pub workspaces: Vec<PlannedWorkspace>,
    pub proposals: Vec<ProposedTask>,
    pub feature_fallbacks: Vec<crate::derive_groups::FeatureFallback>,
    pub tool_checks: Vec<ToolInputCheck>,
    pub clippy_memory: ClippyMemoryPlan,
    pub recommendations: Vec<String>,
    pub consumer_manifest_json: Option<String>,
    pub consumer_manifest_stand_in: bool,
    pub skipped_non_utf8: bool,
    pub tofu_note: Option<velnor_actions_tofu::TofuNote>,
    pub tofu_units: Vec<crate::select_tofu::TofuSelectionUnit>,
    pub affected: crate::affected_inputs::AffectedInputs,
    pub(crate) bun: crate::workflow_task_jobs::bun_inventory::BunInventory,
    pub(crate) dbt: crate::workflow_task_jobs::dbt_inventory::DbtInventory,
    pub(crate) ansible: crate::workflow_task_jobs::ansible_inventory::AnsibleInventory,
}

pub(crate) fn discover(
    root: &Path,
    config: &VelnorConfig,
    catalog: &ToolCatalog,
) -> Result<Discovery, OrchestratorError> {
    discover_inner(root, config, catalog, None)
}

pub(crate) fn discover_with_phase_timings(
    root: &Path,
    config: &VelnorConfig,
    catalog: &ToolCatalog,
    phases: &mut PlanPhaseTimings,
) -> Result<Discovery, OrchestratorError> {
    discover_inner(root, config, catalog, Some(phases))
}

#[path = "discovery_inventories.rs"]
mod task_inventory;
use task_inventory::task_inventories;

fn detect_candidates(index: &FileIndex) -> Vec<velnor_actions_contract::StackCandidate> {
    let mut candidates = Vec::new();
    let mut previous = "";
    for (stack_id, schema, detect) in DETECTORS {
        debug_assert!(previous < stack_id, "registry runs ascending");
        debug_assert_eq!(schema, DETECTION_SCHEMA, "registry schema");
        previous = stack_id;
        candidates.extend(detect(index));
    }
    candidates
}

fn discover_inner(
    root: &Path,
    config: &VelnorConfig,
    catalog: &ToolCatalog,
    mut phases: Option<&mut PlanPhaseTimings>,
) -> Result<Discovery, OrchestratorError> {
    let (index, skipped_non_utf8) = build_file_index(root, &config.discovery.exclude)?;
    let mut candidates = detect_candidates(&index);
    let task_tool_paths = task_tool_paths(config);
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
        Some(phases) => run_inventories_with_phase_timings(
            root,
            &candidates,
            index.files(),
            &mut reads,
            phases,
            catalog,
        ),
        None => run_inventories(root, &candidates, index.files(), &mut reads, catalog),
    };
    let ((outcomes, inventories), tofu_units) = inventory_result?;
    let statuses = check_candidate_outcomes(initial, &outcomes).map_err(|err| {
        OrchestratorError::Detection {
            problem: err.to_string(),
        }
    })?;
    let workspaces = plan_workspaces(root, &index, &statuses, inventories, config)?;
    let resolves = qualify_workspaces(root, &workspaces, catalog)?;
    let rules =
        crate::affected_inputs::resolve_input_rules(config.stacks.rust.as_ref(), &workspaces)?;
    let (mut proposals, fallbacks) = derive_all(config, &index, &workspaces, &statuses)?;
    let mise_checks =
        velnor_actions_mise::discover_checks(root, &config.checks, &config.qualified_tools)
            .map_err(|err| OrchestratorError::Contract {
                problem: err.to_string(),
            })?;
    let native_checks = config.native_checks.clone();
    proposals.extend(mise_checks.iter().map(|item| item.proposal.clone()));
    proposals.extend(
        native_checks
            .iter()
            .map(crate::internal_plan::native_homebrew_checks::proposal)
            .collect::<Result<Vec<_>, _>>()?,
    );
    proposals.sort_by(|a, b| a.task_id.cmp(&b.task_id));
    let clippy_memory = clippy_memory_groups(&proposals);
    let recommendations =
        collect_recommendations(root, config, &index, &workspaces, &tool_checks, &mut reads);
    let (consumer_manifest_json, consumer_manifest_stand_in) =
        consumer_manifest::for_policy(root, config.workflow.policy)?;
    let (bun, dbt, ansible) = task_inventories(root, &index, config)?;
    let gradle_roots = gradle_jobs::analyze_roots(root, &index, config)?;
    Ok(Discovery {
        gradle_roots,
        mise_checks,
        native_checks,
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
        affected: crate::affected_inputs::AffectedInputs { resolves, rules },
        bun,
        dbt,
        ansible,
    })
}

fn task_tool_paths(config: &VelnorConfig) -> Vec<String> {
    let mut paths = config
        .workflow
        .tasks
        .iter()
        .filter_map(|task| match task {
            WorkflowTask::Verification(task) => Some(&task.source),
            WorkflowTask::Build(task) => Some(&task.source),
            WorkflowTask::NativeImage(_) => None,
        })
        .flat_map(|source| {
            [
                source.mise_config.clone(),
                source.mise_lock_path(),
                source.rust_toolchain_path(),
            ]
        })
        .collect::<BTreeSet<_>>();
    for suite in &config.workflow.task_suites {
        paths.insert(suite.source.mise_config.clone());
        paths.insert(suite.source.mise_lock_path());
        paths.insert(suite.source.rust_toolchain_path());
    }
    paths.into_iter().collect()
}

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
        let outcome = profile_for_workspace(
            root,
            index,
            &record,
            config.stacks.rust.as_ref(),
            &config.workflow.preserve_workflows,
        )?;
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
    let explicit_fmt = true;
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

pub(crate) fn workspace_manifest(workspace_root: &str) -> String {
    if workspace_root.is_empty() {
        "Cargo.toml".to_owned()
    } else {
        format!("{workspace_root}/Cargo.toml")
    }
}

pub(crate) fn workspace_lock(workspace_root: &str) -> String {
    if workspace_root.is_empty() {
        "Cargo.lock".to_owned()
    } else {
        format!("{workspace_root}/Cargo.lock")
    }
}
