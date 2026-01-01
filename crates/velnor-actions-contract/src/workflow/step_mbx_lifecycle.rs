//! Cross-step validation for the native MBX cleanup lifecycle.

use super::{Step, StepRole};
use crate::errors::ContractError;

/// Require a native action, its ready guard, and cleanup in one ordered job scope.
pub(super) fn validate_mbx_cleanup_sequence(
    steps: &[Step],
    scope: &str,
) -> Result<(), ContractError> {
    let actions = positions(steps, StepRole::MbxCache);
    let ready = positions(steps, StepRole::MbxVersionCheck);
    let cleanups = positions(steps, StepRole::MbxWorkspaceCleanup);
    if cleanups.is_empty() {
        return Ok(());
    }
    if actions.len() != 1
        || ready.len() != 1
        || cleanups.len() != 1
        || actions[0] >= ready[0]
        || ready[0] >= cleanups[0]
        || cleanups[0] + 1 != steps.len()
    {
        return Err(ContractError::identity(
            "job.steps",
            format!("mbx_cleanup_sequence_mismatch:{scope}"),
        ));
    }
    Ok(())
}

fn positions(steps: &[Step], role: StepRole) -> Vec<usize> {
    steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.role == Some(role))
        .map(|(index, _)| index)
        .collect()
}
