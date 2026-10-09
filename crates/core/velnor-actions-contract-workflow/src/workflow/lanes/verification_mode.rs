//! Shared mode lookup for jobs whose output providers follow workflow lanes.

use super::{Placement, lane_class, placement_for};
use crate::workflow::ir::Job;
use velnor_actions_contract::errors::ContractError;
use velnor_actions_contract_config::config::{ExecutionConfig, ExecutionMode};

/// Resolve verification placement with the same rule used by lane expansion.
///
/// # Errors
///
/// Returns a configuration error for invalid schema-2 execution routing.
pub fn verification_job_mode(
    schema: u32,
    execution: Option<&ExecutionConfig>,
    dispatch: Option<ExecutionMode>,
    job_id: &str,
    job: &Job,
) -> Result<ExecutionMode, ContractError> {
    if schema != 2 {
        return Ok(ExecutionMode::Hosted);
    }
    let execution = execution
        .ok_or_else(|| ContractError::config("config.toml", "execution", "missing_execution"))?;
    Ok(
        match placement_for(execution, dispatch, lane_class(job_id), job_id, job)? {
            Placement::HostedOnly => ExecutionMode::Hosted,
            Placement::ScaleSetOnly => ExecutionMode::ScaleSet,
            Placement::Both => ExecutionMode::Both,
        },
    )
}
