//! Event-time `plan-v1` / `merge-v1` JSON entrypoints (schema 1).

// Obligation identities live beside the planner so `lib.rs` stays untouched.
#[path = "internal_named_lanes.rs"]
mod named_lanes;
#[path = "phase_timing.rs"]
pub(crate) mod phase_timing;
#[path = "plan_obligation.rs"]
pub(crate) mod plan_obligation;
#[path = "plan_response.rs"]
mod plan_response;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{
    ContractError, NamedCheckLane, Plan, PlanBaseline, PlanMatrix, PlanRunner, ProposedTask,
    RunnerSelection, WorkflowEvent, canonical_json_bytes, plan_id_for_run,
};
use velnor_actions_mise::ToolCatalog;

use self::plan_obligation::{GroupInputs, changed_keys, lane_table, member_changed, plan_group};
use crate::OrchestratorError;
use crate::cover::{BaselineInputs, apply_baseline};
use crate::decisions::dedupe_sorted;
use crate::discover::Discovery;
use crate::internal_plan::snapshot::ExecutionSnapshot;
use crate::internal_plan::wire_w2::GroupWire;
use crate::internal_plan::{default_generator_with_phase_timings, plan_packages};
use crate::internal_request::{PlanRequest, checked_plan_request};

pub use self::plan_response::validate_plan_response;
use crate::merge::BaselineManifest;
use crate::prepare::{prepare, prepare_with_phase_timings};
use crate::select::{classify_changed, select_universe};
use crate::select_edges::plan_task_graph;

pub use crate::internal_request::{
    merge_passed, publish_final_report, publish_plan_files, response_path_for, write_request,
    write_request_parts,
};
pub use crate::plan_output_limits::{
    PlanOutputs, plan_outputs, plan_outputs_from_staged_admission, plan_outputs_with_admission,
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
    plan_internal_inner(request_json, None)
}

#[expect(
    clippy::needless_option_as_deref,
    reason = "the Option is intentionally reborrowed across planning stages"
)]
fn plan_internal_inner(
    request_json: &str,
    mut timings: Option<&mut phase_timing::PlanPhaseTimings>,
) -> Result<String, OrchestratorError> {
    let (mut request, root) = checked_plan_request(request_json)?;
    let prep = if let Some(phases) = timings.as_deref_mut() {
        phases.measure_prepare(|phases| prepare_with_phase_timings(&root, phases))?
    } else {
        prepare(&root)?
    };
    validate_qualification_request(&request, &prep.default_branch)?;
    let named_check_lanes = named_lanes::resolve(&prep, request.named_check_lanes.take())?;
    let catalog = ToolCatalog::pinned();
    let mut warnings = Vec::new();
    warnings.extend(crate::evidence::workspace_drift_warnings(
        &prep.root,
        &prep.discovery.workspaces,
        &prep.runner_label,
    ));
    let universe = select_universe(&prep.discovery, &mut warnings)?;
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
        timings.as_deref_mut(),
    )?;
    let manifest = request.baseline_manifest.and_then(|value| {
        serde_json::from_value::<BaselineManifest>(value)
            .inspect_err(|_| {
                plan.warnings
                    .push("baseline_miss:malformed_manifest".to_owned());
            })
            .ok()
    });
    let used_manifest = if request.event == WorkflowEvent::Qualification {
        None
    } else {
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
            manifest,
            &prep.discovery,
            changed.as_ref(),
        )?
    };
    plan.validate().map_err(internal_contract)?;
    check_matrix_budget(&plan.matrix)?;
    let response = plan_response(plan, used_manifest);
    serde_json::to_string(&response).map_err(|err| OrchestratorError::Internal {
        problem: format!("response_encode:{err}"),
    })
}

/// Bind qualification provenance to the prepared repository and checkout.
fn validate_qualification_request(
    request: &PlanRequest,
    default_branch: &str,
) -> Result<(), OrchestratorError> {
    match (request.event, request.qualification.as_ref()) {
        (WorkflowEvent::Qualification, Some(context)) => {
            let repository = request
                .repository
                .as_deref()
                .ok_or_else(|| internal("missing_qualification_repository"))?;
            context
                .validate_for(default_branch, repository, &request.head)
                .map_err(internal_contract)
        }
        (WorkflowEvent::Qualification, None) => Err(internal("missing_qualification_context")),
        (_, Some(_)) => Err(internal("qualification_context_on_wrong_event")),
        (_, None) => Ok(()),
    }
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

/// Build an internal error.
pub(crate) fn internal(problem: &str) -> OrchestratorError {
    OrchestratorError::Internal {
        problem: problem.to_owned(),
    }
}

/// Map a contract error into an internal error.
#[expect(
    clippy::needless_pass_by_value,
    reason = "used directly as a map_err fn"
)]
pub(crate) fn internal_contract(error: ContractError) -> OrchestratorError {
    OrchestratorError::Internal {
        problem: error.to_string(),
    }
}

/// Build the validated plan from the obligation universe.
///
/// Identities attach for every member before changed-work hints and
/// baseline evidence classify dispositions; the execution matrix keeps
/// execute obligations only after that classification.
#[expect(
    clippy::too_many_arguments,
    reason = "one call site threads plan scope, universe, classification, and per-call timings"
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
    mut timings: Option<&mut phase_timing::PlanPhaseTimings>,
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
    #[expect(
        clippy::needless_option_as_deref,
        reason = "the original Option must remain available after this call"
    )]
    let generator = default_generator_with_phase_timings(timings.as_deref_mut());
    let snapshot = ExecutionSnapshot::build(discovery);
    let mut reads = velnor_actions_tofu::FileCache::new();
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
        schema: Plan::SCHEMA,
        run_key: request.run_key.clone(),
        plan_id: plan_id_for_run(&request.run_key).map_err(internal_contract)?,
        base: request.base.clone(),
        head: request.head.clone(),
        event: request.event,
        qualification: request.qualification.clone(),
        runner: PlanRunner {
            label: label.to_owned(),
            selection,
        },
        trust: velnor_actions_contract::trust_for_event(request.event),
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
