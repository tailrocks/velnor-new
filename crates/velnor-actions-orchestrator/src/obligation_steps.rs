//! Closed action adapters preserve normal obligation identity and failure reports.

use velnor_actions_contract::{CrateJob, CrateObligation, Stack, Step, StepId};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::steps::INTERNAL_OP_ENV;

use crate::OrchestratorError;
use crate::action_report::{ACTION_BEGIN_OP, ACTION_ID_ENV, ACTION_OUTCOME_ENV, ACTION_REPORT_OP};
use crate::matrix_step::{helper_path_for_version, obligation_identity_env, task_step_env};

/// Select the reviewed native adapter; ordinary obligations retain shell reports.
pub(crate) fn steps(
    model: &CrateJob,
    obligation: &CrateObligation,
    catalog: &ToolCatalog,
    downstream: &[String],
    cap: Option<u32>,
) -> Result<Option<Vec<Step>>, OrchestratorError> {
    if !model.job_id.starts_with("workload-") || model.configuration != "docker_build" {
        return Ok(None);
    }
    if obligation.kind != "build"
        || model.obligations.len() != 1
        || !downstream.is_empty()
        || !obligation.gated_by.is_empty()
        || model.obligations.first() != Some(obligation)
    {
        return Err(crate::internal::internal(
            "unsupported_action_obligation_shape",
        ));
    }
    let matrix_id = velnor_actions_contract::matrix_id_for_task_group(
        Stack::Workload.id(),
        &obligation.task_id,
    )?;
    let mut identity = obligation_identity_env(
        &obligation.task_id,
        &obligation.task_digest,
        &matrix_id,
        &obligation.matrix_key,
        cap,
    );
    let id = format!("velnor-action-{}", obligation.matrix_key);
    identity.insert(ACTION_ID_ENV.to_owned(), id.clone());
    let uncovered = crate::covered_tasks::skip_condition(&obligation.task_id)?;
    let always = format!("always() && ({uncovered})");
    let env = task_step_env(catalog, &identity, false)?;
    let mut begin_env = env.clone();
    begin_env.insert(INTERNAL_OP_ENV.to_owned(), ACTION_BEGIN_OP.to_owned());
    let mut begin = velnor_actions_workflow_renderer::shell_step(
        "Begin container obligation",
        vec![helper_path_for_version()],
        begin_env,
    )?;
    begin.condition = Some(always.clone());
    let mut result = vec![begin];
    let mut action = crate::workloads::cache::docker::build_step(model, false)?;
    action.id = Some(StepId::new(&id)?);
    action.condition = Some(format!("success() && ({uncovered})"));
    result.push(action);
    let mut after_env = env;
    after_env.insert(INTERNAL_OP_ENV.to_owned(), ACTION_REPORT_OP.to_owned());
    after_env.insert(
        ACTION_OUTCOME_ENV.to_owned(),
        format!("${{{{ steps.{id}.outcome }}}}"),
    );
    let mut after = velnor_actions_workflow_renderer::shell_step(
        "Report container obligation",
        vec![helper_path_for_version()],
        after_env,
    )?;
    after.condition = Some(always);
    result.push(after);
    // A supported export solves the same local builder again only after the
    // producer's source-bound successful reports have been written. No repo
    // shell receives transport credentials, and failed reports prevent export.
    let mut export = crate::workloads::cache::docker::build_step(model, true)?;
    export.id = Some(StepId::new(&format!("{id}-export"))?);
    let gate = export
        .condition
        .take()
        .ok_or_else(|| crate::internal::internal("action_cache_trust_gate_missing"))?;
    export.condition = Some(format!("({gate}) && ({uncovered})"));
    result.push(export);
    Ok(Some(result))
}

#[cfg(test)]
#[path = "obligation_steps_tests.rs"]
mod tests;
