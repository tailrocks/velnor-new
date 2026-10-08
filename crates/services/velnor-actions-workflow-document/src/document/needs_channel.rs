//! Final-gate environment channels derived from the finalized workflow graph.

use std::collections::BTreeMap;

use velnor_actions_contract_config::ExecutionMode;
use velnor_actions_contract_workflow::{Job, NeedsConclusions, TaskReportProducerInventory};
use velnor_actions_workflow_jobs::context::FINAL_JOB_ID;
use velnor_actions_workflow_steps::RenderError;

/// Build existing needs channels and the explicit-Both producer inventory.
pub(super) fn needs_channel_envs(
    jobs: &BTreeMap<String, Job>,
    execution_mode: Option<ExecutionMode>,
) -> Result<Vec<(String, String)>, RenderError> {
    if !jobs.contains_key(FINAL_JOB_ID) {
        return Ok(Vec::new());
    }
    let conclusions =
        NeedsConclusions::from_finalized_jobs(FINAL_JOB_ID, jobs).map_err(RenderError::Contract)?;
    if !conclusions.gate_matches(jobs) {
        return Err(RenderError::InvalidWorkflow(
            "needs_inventory_gate_mismatch".to_owned(),
        ));
    }
    let mut env = vec![conclusions.channel_env(), conclusions.expected_env()];
    if let Some(inventory) =
        TaskReportProducerInventory::from_finalized_jobs(execution_mode, FINAL_JOB_ID, jobs)
            .map_err(RenderError::Contract)?
    {
        env.push(inventory.expected_env().map_err(RenderError::Contract)?);
    }
    Ok(env)
}
