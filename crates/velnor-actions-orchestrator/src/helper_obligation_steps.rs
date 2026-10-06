//! Source-owner qualified obligation framing with always-on terminal evidence.

use velnor_actions_contract::{
    CompiledSourceHelper, CrateObligation, HelperObligationDescriptor, Stack, Step, StepId,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::steps::INTERNAL_OP_ENV;

use crate::OrchestratorError;
use crate::helper_obligation_report::{
    HELPER_BEGIN_OP, HELPER_ID_ENV, HELPER_OUTCOME_ENV, HELPER_REPORT_OP,
};
use crate::matrix_step::{helper_path_for_version, obligation_identity_env, task_step_env};

/// Persist the exact compiled invocation and environment in the planned entry.
pub(crate) fn descriptor(
    record: &CompiledSourceHelper,
    matrix_key: &str,
) -> Result<serde_json::Value, OrchestratorError> {
    let binding = HelperObligationDescriptor::from_compiled(record, matrix_key)?;
    serde_json::to_value(binding)
        .map_err(|error| crate::internal::internal(&format!("helper_descriptor_encoding:{error}")))
}

/// Frame a helper without adding authority to its owner-qualified environment.
pub(crate) fn steps(
    obligation: &CrateObligation,
    catalog: &ToolCatalog,
    downstream: &[String],
    cap: Option<u32>,
    record: &CompiledSourceHelper,
) -> Result<Vec<Step>, OrchestratorError> {
    let id = helper_id(&obligation.matrix_key)?;
    let mut env = report_env(obligation, catalog, downstream, cap, &id)?;
    let uncovered = crate::covered_tasks::skip_condition(&obligation.task_id)?;
    let always = format!("always() && ({uncovered})");
    let mut begin_env = env.clone();
    begin_env.insert(INTERNAL_OP_ENV.to_owned(), HELPER_BEGIN_OP.to_owned());
    let mut begin = velnor_actions_workflow_renderer::shell_step(
        &format!("Begin {} obligation", obligation.step_name),
        vec![helper_path_for_version()],
        begin_env,
    )?;
    begin.condition = Some(always.clone());
    let mut helper = velnor_actions_workflow_renderer::source_helper::source_helper_step(
        &obligation.step_name,
        record,
        record.environment().clone(),
    )?;
    helper.id = Some(id.clone());
    helper.condition = Some(format!("success() && ({uncovered})"));
    env.insert(INTERNAL_OP_ENV.to_owned(), HELPER_REPORT_OP.to_owned());
    env.insert(
        HELPER_OUTCOME_ENV.to_owned(),
        format!("${{{{ steps.{}.outcome }}}}", id.as_str()),
    );
    let mut report = velnor_actions_workflow_renderer::shell_step(
        &format!("Report {} obligation", obligation.step_name),
        vec![helper_path_for_version()],
        env,
    )?;
    report.condition = Some(always);
    Ok(vec![begin, helper, report])
}

fn helper_id(matrix_key: &str) -> Result<StepId, OrchestratorError> {
    velnor_actions_contract::validate_matrix_key(matrix_key)?;
    Ok(StepId::new(&format!("velnor-helper-{matrix_key}"))?)
}

fn report_env(
    obligation: &CrateObligation,
    catalog: &ToolCatalog,
    downstream: &[String],
    cap: Option<u32>,
    id: &StepId,
) -> Result<std::collections::BTreeMap<String, String>, OrchestratorError> {
    let stack = crate::extension_schemas::task_stack_segment(&obligation.task_id)
        .and_then(Stack::from_id)
        .ok_or_else(|| crate::internal::internal("helper_obligation_stack_unknown"))?;
    let matrix_id =
        velnor_actions_contract::matrix_id_for_task_group(stack.id(), &obligation.task_id)?;
    let mut identity = obligation_identity_env(
        &obligation.task_id,
        &obligation.task_digest,
        &matrix_id,
        &obligation.matrix_key,
        cap,
    );
    identity.insert(HELPER_ID_ENV.to_owned(), id.as_str().to_owned());
    if !downstream.is_empty() {
        identity.insert(
            crate::task_report::DOWNSTREAM_IDS_ENV.to_owned(),
            downstream.join(","),
        );
    }
    task_step_env(catalog, &identity, false)
}

#[cfg(test)]
#[path = "helper_obligation_steps_tests.rs"]
mod tests;
