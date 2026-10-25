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
    ContractError, NamedCheckLane, ObligationDecision, Plan, PlanMatrix, PlanRunner, ProposedTask,
    RunnerSelection, WorkflowEvent, canonical_json_bytes, plan_id_for_run,
};
use velnor_actions_mise::ToolCatalog;

use self::plan_obligation::{GroupInputs, changed_keys, lane_table, member_changed, plan_group};
use crate::OrchestratorError;
use crate::decisions::dedupe_sorted;
use crate::discover::Discovery;
use crate::internal_plan::snapshot::ExecutionSnapshot;
use crate::internal_plan::wire_w2::GroupWire;
use crate::internal_plan::{default_generator_with_phase_timings, plan_packages};
use crate::internal_request::{PlanRequest, checked_plan_request};

pub use self::plan_response::validate_plan_response;
use crate::prepare::{prepare, prepare_with_phase_timings};
use crate::select::{ChangeEvidence, classify_changed, select_universe};
use crate::select_edges::plan_task_graph;

pub use crate::internal_request::{
    merge_passed, publish_final_report, publish_plan_files, response_path_for, write_request,
    write_request_parts,
};
pub use crate::plan_output_limits::{
    PlanOutputs, plan_outputs, plan_outputs_from_staged_admission, plan_outputs_with_admission,
};

pub(crate) const SCHEMA: u32 = 1;

pub(crate) const MATRIX_BUDGET_BYTES: usize = 4_194_304;

pub const REQUEST_FILE_ENV: &str = "VELNOR_REQUEST_FILE";
pub const WRITE_REQUEST_OP: &str = "write-request-v1";
pub const PLAN_OP: &str = "plan-v1";
pub const MERGE_OP: &str = "merge-v1";

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PlanResponse {
    pub(crate) schema: u32,
    pub(crate) plan: Plan,
    pub(crate) matrix: PlanMatrix,
}

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
    let catalog = &prep.catalog;
    let mut warnings = Vec::new();
    warnings.extend(crate::evidence::workspace_drift_warnings(
        &prep.root,
        &prep.discovery.workspaces,
        &prep.runner_label,
    ));
    let universe = select_universe(&prep.discovery, &mut warnings)?;
    let mut evidence = ChangeEvidence::default();
    let changed = classify_changed(
        &prep.root,
        request.event,
        request.base.as_deref(),
        &request.head,
        &prep.discovery,
        &mut warnings,
        &mut evidence,
    );
    let mut plan = build_plan(
        &request,
        &prep.discovery,
        &prep.root,
        &universe,
        changed.as_ref(),
        &prep.runner_label,
        prep.runner_selection,
        catalog,
        &named_check_lanes,
        warnings,
        timings.as_deref_mut(),
    )?;
    plan.jobs.extend(crate::select::images::dispositions(
        &prep.root,
        &prep.config.images,
        &prep.discovery,
        request.event,
        changed.as_ref(),
        &evidence,
    ));
    for dispositions in [
        crate::select::ansible::dispositions,
        crate::select::bun::dispositions,
        crate::select::dbt::dispositions,
    ] {
        plan.jobs.extend(dispositions(
            &prep.config,
            &prep.discovery,
            request.event,
            changed.as_ref(),
            &evidence,
        ));
    }
    plan.jobs
        .sort_by(|left, right| left.job_id.cmp(&right.job_id));
    plan.validate().map_err(internal_contract)?;
    check_matrix_budget(&plan.matrix)?;
    let response = plan_response(plan);
    serde_json::to_string(&response).map_err(|err| OrchestratorError::Internal {
        problem: format!("response_encode:{err}"),
    })
}

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

fn plan_response(plan: Plan) -> PlanResponse {
    PlanResponse {
        schema: SCHEMA,
        matrix: plan.matrix.clone(),
        plan,
    }
}

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

pub(crate) fn check_schema(schema: u32) -> Result<(), OrchestratorError> {
    if schema == SCHEMA {
        Ok(())
    } else {
        Err(internal(&format!("unsupported_schema:{schema}")))
    }
}

pub(crate) fn internal(problem: &str) -> OrchestratorError {
    OrchestratorError::Internal {
        problem: problem.to_owned(),
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "used directly as a map_err fn"
)]
pub(crate) fn internal_contract(error: ContractError) -> OrchestratorError {
    OrchestratorError::Internal {
        problem: error.to_string(),
    }
}

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
    #[expect(
        clippy::needless_option_as_deref,
        reason = "the original Option must remain available after this call"
    )]
    let generator = default_generator_with_phase_timings(timings.as_deref_mut());
    let snapshot = ExecutionSnapshot::build(discovery);
    let job_owners = crate::crate_job_ids::JobOwners::new(&discovery.proposals)?;
    let mut reads = velnor_actions_tofu::FileCache::new();
    for task in universe {
        let wire = GroupWire {
            event: request.event,
            generator: &generator,
        };
        let (obligation, entry) = plan_group(
            &GroupInputs {
                discovery,
                job_owners: &job_owners,
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
    let needs = crate::plan_dispositions::job_needs(universe, &job_owners)?;
    let jobs =
        crate::plan_dispositions::finalize_dispositions(&mut obligations, &mut entries, &needs);
    let selected_ids = executing_units(universe, &obligations);
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
        generator,
        packages: plan_packages(discovery, &selected_ids),
        obligations,
        matrix: PlanMatrix { include: entries },
        task_ids,
        jobs,
        warnings,
        edges,
    })
}

fn executing_units<'a>(
    universe: &[&'a ProposedTask],
    obligations: &[velnor_actions_contract::PlanObligation],
) -> BTreeSet<&'a str> {
    let executing: BTreeSet<&str> = obligations
        .iter()
        .filter(|ob| ob.decision != ObligationDecision::NotAffected)
        .map(|ob| ob.task_id.as_str())
        .collect();
    universe
        .iter()
        .filter(|task| executing.contains(task.task_id.as_str()))
        .map(|task| task.identity.unit_id.as_str())
        .collect()
}
