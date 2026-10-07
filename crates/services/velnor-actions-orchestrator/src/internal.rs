//! Event-time `plan-v1` / `merge-v1` JSON entrypoints (schema 1).

// Obligation identities live beside the planner so `lib.rs` stays untouched.
pub(crate) mod plan_obligation;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{canonical_json_bytes, parse_strict_json, plan_id_for_run};
use velnor_actions_contract_config::{ExecutionMode, RunnerSelection};
use velnor_actions_contract_planning::ProposedTask;
use velnor_actions_contract_workflow::{
    NamedCheckLane, Plan, PlanBaseline, PlanMatrix, PlanRunner, WorkflowEvent, named_check_lanes,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_orchestrator_core::decisions::dedupe_sorted;
use velnor_actions_orchestrator_core::{OrchestratorError, internal, internal_contract};

use self::plan_obligation::{GroupInputs, changed_keys, lane_table, member_changed, plan_group};
use crate::cover::{BaselineInputs, apply_baseline};
use crate::internal_request::resolve_run_key;
use crate::merge::BaselineManifest;
use crate::prepare::prepare;
use velnor_actions_orchestrator_discovery::discover::Discovery;
use velnor_actions_orchestrator_discovery::select_edges::plan_task_graph;
use velnor_actions_orchestrator_graph::internal_plan::snapshot::ExecutionSnapshot;
use velnor_actions_orchestrator_graph::internal_plan::wire_w2::GroupWire;
use velnor_actions_orchestrator_graph::internal_plan::{default_generator, plan_packages};
use velnor_actions_orchestrator_selection::select::{
    classify_changed, select_universe, verify_checkout,
};

pub use crate::internal_request::{
    PlanOutputs, merge_passed, plan_outputs, publish_final_report, publish_plan_files,
    response_path_for, write_request, write_request_parts,
};

/// Schema version accepted by both internal entrypoints.
pub(crate) const SCHEMA: u32 = 1;

/// Maximum canonical `matrix.json` artifact bytes; oversize errors, never
/// truncates. Job outputs have a separate UTF-16 aggregate budget.
pub(crate) const MATRIX_BUDGET_BYTES: usize = 524_288;

/// Env key carrying the exact request-file path.
pub const REQUEST_FILE_ENV: &str = "VELNOR_REQUEST_FILE";
/// Write-request operation tag.
pub const WRITE_REQUEST_OP: &str = "write-request-v1";
/// Plan operation tag.
pub const PLAN_OP: &str = "plan-v1";
/// Merge operation tag.
pub const MERGE_OP: &str = "merge-v1";

/// `plan-v1` request: run scope plus optional repo root.
///
/// The request carries no generator identity: the plan always names the
/// running binary, so a hand-written request can never claim a release
/// pin for a source build. Unknown fields (including `generator`)
/// reject via `deny_unknown_fields`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanRequest {
    /// Request schema; must be 1.
    schema: u32,
    /// Consuming operation; must be `plan-v1` when present.
    #[serde(default)]
    op: Option<String>,
    /// Run key; empty derives from the GitHub environment.
    #[serde(default)]
    run_key: String,
    /// Base commit or null.
    base: Option<String>,
    /// Head commit.
    head: String,
    /// Triggering event.
    event: WorkflowEvent,
    /// Repository root override; defaults to the resolved root.
    #[serde(default)]
    root: Option<PathBuf>,
    /// Trusted baseline evidence for coverage classification.
    #[serde(default)]
    baseline_manifest: Option<serde_json::Value>,
    /// Runner-owned repository slug (`owner/repo`) for provenance.
    ///
    /// The request writer captures this from `GITHUB_REPOSITORY`; the
    /// planner never reads ambient env itself, so classification stays
    /// a pure function of request plus checkout. Absent means a local
    /// run: the git origin is the fallback.
    #[serde(default)]
    repository: Option<String>,
    /// Exact named-check job identities carried by the generated workflow.
    #[serde(default)]
    named_check_lanes: Option<BTreeMap<String, Vec<NamedCheckLane>>>,
}

/// `plan-v1` response: schema plus plan and matrix copies.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PlanResponse {
    /// Response schema; always 1.
    pub(crate) schema: u32,
    /// Affected plan.
    pub(crate) plan: Plan,
    /// Matrix copy, byte-identical to the embedded matrix.
    pub(crate) matrix: PlanMatrix,
    /// Trusted manifest behind covered obligations, when any covered.
    ///
    /// The plan artifact stages these bytes as `baseline.json` so the
    /// merge revalidates covered claims against the exact evidence the
    /// planner used; absent when nothing covered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) baseline_manifest: Option<BaselineManifest>,
}

/// Compute the affected plan plus matrix for one event (schema-1 JSON).
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for malformed requests and
/// discovery, selection, or validation failures.
pub fn plan_internal(request_json: &str) -> Result<String, OrchestratorError> {
    let envelope = parse_strict_json(request_json).map_err(internal_contract)?;
    let mut request: PlanRequest =
        serde_json::from_value(envelope).map_err(|err| OrchestratorError::Internal {
            problem: format!("malformed_request:{err}"),
        })?;
    check_schema(request.schema)?;
    if request.op.as_deref().is_some_and(|op| op != PLAN_OP) {
        return Err(internal("op_mismatch"));
    }
    let run_key = resolve_run_key(Some(request.run_key.as_str()))?;
    request.run_key = run_key;
    if request.head.trim().is_empty() {
        return Err(internal("empty_head"));
    }
    let root = plan_root(request.root.as_deref())?;
    verify_checkout(&root, request.event, &request.head)?;
    let prep = prepare(&root)?;
    let named_check_lanes = resolve_named_check_lanes(&prep, request.named_check_lanes.take())?;
    let catalog = ToolCatalog::pinned();
    let mut warnings = Vec::new();
    warnings.extend(
        velnor_actions_orchestrator_discovery::evidence::workspace_drift_warnings(
            &prep.root,
            &prep.discovery.workspaces,
            &prep.runner_label,
        ),
    );
    let universe = select_universe(&prep.discovery, &mut warnings);
    let changed = classify_changed(
        &prep.root,
        request.event,
        request.base.as_deref(),
        &request.head,
        &prep.discovery,
        &mut warnings,
    );
    let mut plan = build_plan(
        &request,
        &prep.discovery,
        &prep.root,
        &universe,
        changed.as_ref(),
        &prep.runner_label,
        prep.runner_selection,
        &catalog,
        &named_check_lanes,
        warnings,
    )?;
    let manifest = request.baseline_manifest.and_then(|value| {
        serde_json::from_value::<BaselineManifest>(value)
            .inspect_err(|_| {
                plan.warnings
                    .push("baseline_miss:malformed_manifest".to_owned());
            })
            .ok()
    });
    apply_baseline(
        &mut plan,
        request.event,
        BaselineInputs {
            branch: &prep.default_branch,
            root: &prep.root,
            workflow: velnor_actions_workflow_renderer::render::WORKFLOW_PATH,
            catalog: &catalog,
            repository: request.repository.as_deref(),
        },
        manifest.clone(),
        &prep.discovery,
        changed.as_ref(),
    )?;
    plan.validate().map_err(internal_contract)?;
    check_matrix_budget(&plan.matrix)?;
    let response = plan_response(plan, manifest);
    serde_json::to_string(&response).map_err(|err| OrchestratorError::Internal {
        problem: format!("response_encode:{err}"),
    })
}

fn resolve_named_check_lanes(
    prep: &crate::prepare::GenerationPreparation,
    supplied: Option<BTreeMap<String, Vec<NamedCheckLane>>>,
) -> Result<BTreeMap<String, Vec<NamedCheckLane>>, OrchestratorError> {
    let candidates = [
        None,
        Some(ExecutionMode::Hosted),
        Some(ExecutionMode::ScaleSet),
        Some(ExecutionMode::Both),
    ];
    let default =
        named_check_lanes(&prep.workflow.ir, &prep.config, None).map_err(internal_contract)?;
    let Some(supplied) = supplied else {
        return Ok(default);
    };
    for dispatch in candidates {
        let expected = named_check_lanes(&prep.workflow.ir, &prep.config, dispatch)
            .map_err(internal_contract)?;
        if supplied == expected {
            return Ok(supplied);
        }
    }
    Err(internal("named_check_lane_contract_mismatch"))
}

/// Assemble the `plan-v1` response, staging trusted bytes when covered.
///
/// The manifest rides along only behind covered obligations: a
/// rejected manifest must never reach the plan artifact.
fn plan_response(plan: Plan, manifest: Option<BaselineManifest>) -> PlanResponse {
    let covered = crate::covered_tasks::plan_has_covered(&plan);
    PlanResponse {
        schema: SCHEMA,
        matrix: plan.matrix.clone(),
        plan,
        baseline_manifest: covered.then_some(manifest).flatten(),
    }
}

/// Reject a matrix whose canonical bytes exceed the budget.
fn check_matrix_budget(matrix: &PlanMatrix) -> Result<(), OrchestratorError> {
    let bytes = canonical_json_bytes(matrix).map_err(internal_contract)?;
    if bytes.len() > MATRIX_BUDGET_BYTES {
        return Err(internal(&format!(
            "matrix_budget_exceeded:{}:broaden selection or reduce matrix entries",
            bytes.len()
        )));
    }
    Ok(())
}

/// Reject any schema other than 1.
pub(crate) fn check_schema(schema: u32) -> Result<(), OrchestratorError> {
    if schema == SCHEMA {
        Ok(())
    } else {
        Err(internal(&format!("unsupported_schema:{schema}")))
    }
}

/// Repository root: explicit override or resolved from the current directory.
fn plan_root(override_root: Option<&Path>) -> Result<PathBuf, OrchestratorError> {
    if let Some(root) = override_root {
        return root
            .canonicalize()
            .map_err(|err| OrchestratorError::io(root.display().to_string(), err.to_string()));
    }
    let cwd = std::env::current_dir().map_err(|err| OrchestratorError::RootDiscovery {
        problem: err.to_string(),
    })?;
    velnor_actions_orchestrator_core::root::resolve_root(&cwd)
}

/// Build the validated plan from the obligation universe.
///
/// Identities attach for every member before changed-work hints and
/// baseline evidence classify dispositions; the execution matrix keeps
/// execute obligations only after that classification.
#[expect(
    clippy::too_many_arguments,
    reason = "one call site threads plan scope plus universe plus classification"
)]
fn build_plan(
    request: &PlanRequest,
    discovery: &Discovery,
    root: &Path,
    universe: &[&ProposedTask],
    changed: Option<&BTreeSet<String>>,
    label: &str,
    selection: RunnerSelection,
    catalog: &ToolCatalog,
    named_check_lanes: &BTreeMap<String, Vec<NamedCheckLane>>,
    warnings: Vec<String>,
) -> Result<Plan, OrchestratorError> {
    let mut obligations = Vec::with_capacity(universe.len());
    let mut entries = Vec::with_capacity(universe.len());
    let mut task_ids = Vec::with_capacity(universe.len());
    let mut digests = BTreeMap::new();
    let lanes = lane_table(universe);
    let keys = changed
        .map(|set| changed_keys(universe, set))
        .unwrap_or_default();
    // The running binary names itself: no request override, no lock
    // fill, so a source build can never emit a release-pinned identity.
    let generator = default_generator();
    let snapshot = ExecutionSnapshot::build(discovery);
    let mut reads = velnor_actions_tofu_core::FileCache::new();
    for task in universe {
        let wire = GroupWire {
            event: request.event,
            generator: &generator,
        };
        let (obligation, entry) = plan_group(
            &GroupInputs {
                discovery,
                task,
                run_key: &request.run_key,
                label,
                lane: lanes.get(&task.task_id).copied().unwrap_or(0),
                catalog,
                wire,
                changed: member_changed(task, changed, &keys),
                snapshot: &snapshot,
                root,
                named_check_lanes,
            },
            &mut reads,
        )?;
        task_ids.push(task.task_id.clone());
        digests.insert(task.task_id.clone(), obligation.input_digest.clone());
        obligations.push(obligation);
        entries.extend(entry);
    }
    obligations.sort_by(|left, right| left.task_id.cmp(&right.task_id));
    entries.sort_by(|left, right| left.id.cmp(&right.id));
    task_ids.sort();
    let edges = plan_task_graph(universe, &lanes, &digests).map_err(internal_contract)?;
    let (_, duplicates) = dedupe_sorted(&task_ids);
    if let Some(dup) = duplicates.first() {
        return Err(internal(&format!("duplicate_task_id:{dup}")));
    }
    let selected_ids: BTreeSet<&str> = universe
        .iter()
        .map(|task| task.identity.unit_id.as_str())
        .collect();
    Ok(Plan {
        schema: SCHEMA,
        run_key: request.run_key.clone(),
        plan_id: plan_id_for_run(&request.run_key).map_err(internal_contract)?,
        base: request.base.clone(),
        head: request.head.clone(),
        event: request.event,
        runner: PlanRunner {
            label: label.to_owned(),
            selection,
        },
        trust: velnor_actions_contract_workflow::trust_for_event(request.event),
        baseline: PlanBaseline::unavailable(Some("baseline_lookup_deferred"))?,
        generator,
        packages: plan_packages(discovery, &selected_ids),
        obligations,
        matrix: PlanMatrix { include: entries },
        task_ids,
        warnings,
        edges,
    })
}
