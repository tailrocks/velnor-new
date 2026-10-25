//! Task input identities from the current source tree.

use std::path::Path;

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{
    MatrixEntry, NamedCheckLane, PlanGenerator, PlanObligation, ProposedTask, Stack,
};
use velnor_actions_mise::ToolCatalog;

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::internal::plan_obligation::{self, identity::TaskIdentityDerivationInputs};
use crate::internal::{internal, internal_contract};
use crate::internal_plan::identities::{platform_id_for_group, toolchain_inputs_for};
use crate::internal_plan::snapshot::ExecutionSnapshot;
use crate::vectors::task_argv;

#[cfg(test)]
#[path = "qualification_cache_runtime_closure_tests.rs"]
mod tests;

/// Source and plan inputs for a check of one lane after its command ends.
pub(crate) struct PostTaskClosureContext<'a> {
    /// Repository checkout.
    pub(crate) root: &'a Path,
    /// Source inventory read after the command ends.
    pub(crate) discovery: &'a Discovery,
    /// Resolved tool versions.
    pub(crate) catalog: &'a ToolCatalog,
    /// Generator identity recorded by the plan.
    pub(crate) generator: &'a PlanGenerator,
    /// Current run key.
    pub(crate) run_key: &'a str,
    /// Logical runner selected by the plan.
    pub(crate) logical_runner_label: &'a str,
}

/// Digests from source and task data available after execution.
///
/// These fields contain values made after task execution.
/// Callers compare these values with the plan values.
/// They give an error if the values do not agree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ObservedLaneClosure {
    /// Matrix lane selected from the plan.
    pub(crate) matrix_key: String,
    /// Logical task selected from the plan.
    pub(crate) task_id: String,
    /// Task command and toolchain digest from the current proposal.
    pub(crate) observed_task_digest: String,
    /// Input identity from current task and source data.
    pub(crate) observed_input_digest: String,
    /// Input closure digest from current source data.
    pub(crate) observed_closure_digest: String,
}

/// Make task and input identities again for one planned lane after execution.
///
/// `discovery` must come from a new source scan after the task command ends.
/// Rust and `ToFu` use the same identity method as plan creation. Named checks
/// use the plan-time lane method with the current check definition and task
/// settings. Native checks cannot request cache-closure evidence because
/// their plan explicitly disables task reuse. The logical runner label is the
/// plan label. Physical matrix lanes can use different runner labels.
/// # Errors
pub(crate) fn recompute_post_task_closure(
    context: &PostTaskClosureContext<'_>,
    requested_matrix_key: &str,
    planned_entry: &MatrixEntry,
) -> Result<ObservedLaneClosure, OrchestratorError> {
    if requested_matrix_key != planned_entry.matrix_key {
        return Err(internal("qualification_closure_matrix_key_mismatch"));
    }
    let task = find_task(context.discovery, &planned_entry.task_id)?;
    if task.stack_id != planned_entry.stack_id {
        return Err(internal("qualification_closure_stack_mismatch"));
    }
    match Stack::require_known(&task.stack_id).map_err(internal_contract)? {
        Stack::Mise => recompute_named_check(context, requested_matrix_key, planned_entry, task),
        Stack::Rust | Stack::Tofu => recompute_adapter_task(context, requested_matrix_key, task),
        Stack::Native => Err(internal("qualification_closure_native_not_cacheable")),
    }
}

/// Compare current values with the planned entry and obligation.
/// # Errors
pub(crate) fn require_plan_match(
    observed: &ObservedLaneClosure,
    entry: &MatrixEntry,
    obligation: &PlanObligation,
) -> Result<(), OrchestratorError> {
    if observed.matrix_key != entry.matrix_key
        || observed.task_id != entry.task_id
        || observed.task_id != obligation.task_id
    {
        return Err(internal("qualification_closure_lane_mismatch"));
    }
    if observed.observed_task_digest != entry.task_digest
        || obligation.task_digest != entry.task_digest
    {
        return Err(internal("qualification_closure_task_digest_mismatch"));
    }
    if observed.observed_input_digest != entry.input_digest
        || observed.observed_input_digest != obligation.input_digest
    {
        return Err(internal("qualification_closure_input_digest_mismatch"));
    }
    if observed.observed_closure_digest != obligation.closure_digest {
        return Err(internal("qualification_closure_digest_mismatch"));
    }
    Ok(())
}

fn find_task<'a>(
    discovery: &'a Discovery,
    task_id: &str,
) -> Result<&'a ProposedTask, OrchestratorError> {
    let mut matches = discovery
        .proposals
        .iter()
        .filter(|task| task.task_id == task_id);
    let task = matches
        .next()
        .ok_or_else(|| internal("qualification_closure_task_missing"))?;
    if matches.next().is_some() {
        return Err(internal("qualification_closure_task_ambiguous"));
    }
    Ok(task)
}

fn recompute_adapter_task(
    context: &PostTaskClosureContext<'_>,
    matrix_key: &str,
    task: &ProposedTask,
) -> Result<ObservedLaneClosure, OrchestratorError> {
    let toolchain_inputs =
        toolchain_inputs_for(task, context.catalog).map_err(internal_contract)?;
    let toolchain = velnor_actions_contract::cachekey::toolchain_id(&toolchain_inputs)
        .map_err(internal_contract)?;
    let argv = task_argv(task, context.catalog)?;
    let platform_id =
        platform_id_for_group(context.logical_runner_label, task).map_err(internal_contract)?;
    let snapshot = ExecutionSnapshot::build(context.discovery);
    let identity = plan_obligation::identity::derive_task_identity(
        &TaskIdentityDerivationInputs {
            root: context.root,
            discovery: context.discovery,
            task,
            argv: &argv,
            toolchain: &toolchain,
            platform_id: &platform_id,
            generator: context.generator,
            snapshot: &snapshot,
        },
        &mut velnor_actions_tofu::FileCache::new(),
    )?;
    let task_digest = plan_obligation::task_digest(&task.task_id, &argv, &toolchain)
        .map_err(internal_contract)?;
    Ok(ObservedLaneClosure {
        matrix_key: matrix_key.to_owned(),
        task_id: task.task_id.clone(),
        observed_task_digest: task_digest,
        observed_input_digest: identity.input_digest,
        observed_closure_digest: identity.closure_digest,
    })
}

fn recompute_named_check(
    context: &PostTaskClosureContext<'_>,
    matrix_key: &str,
    entry: &MatrixEntry,
    task: &ProposedTask,
) -> Result<ObservedLaneClosure, OrchestratorError> {
    let item = crate::internal_plan::named_checks::discovered(context.discovery, task)
        .map_err(internal_contract)?;
    let lane = NamedCheckLane {
        variant: entry.lane_variant,
        job_id: entry.job_id.clone(),
        runs_on: Some(entry.planned_platform.runs_on.clone()),
    };
    let (obligation, mut entries) = crate::internal_plan::named_checks::plan::derive_lanes(
        context.root,
        item,
        context.run_key,
        context.generator,
        context.catalog,
        &[lane],
    )?;
    if entries.len() != 1 {
        return Err(internal("qualification_closure_named_lane_count"));
    }
    let observed_entry = entries
        .pop()
        .ok_or_else(|| internal("qualification_closure_named_lane_missing"))?;
    if observed_entry.matrix_key != matrix_key
        || observed_entry.task_id != entry.task_id
        || observed_entry.job_id != entry.job_id
    {
        return Err(internal("qualification_closure_named_lane_mismatch"));
    }
    Ok(ObservedLaneClosure {
        matrix_key: matrix_key.to_owned(),
        task_id: obligation.task_id,
        observed_task_digest: obligation.task_digest,
        observed_input_digest: obligation.input_digest,
        observed_closure_digest: obligation.closure_digest,
    })
}
