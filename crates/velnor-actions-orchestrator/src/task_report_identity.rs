//! Bind the compiled execution frame before accepting plan evidence.
use velnor_actions_contract::{Plan, validate_digest, validate_task_id};

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};

/// Require the baked task digest to name this exact planned obligation.
/// Coverage shortcuts obey the same binding as executed report writers.
pub(crate) fn validate_expected_digest(
    plan: &Plan,
    task_id: &str,
    expected_digest: &str,
) -> Result<(), OrchestratorError> {
    validate_task_id(task_id).map_err(internal_contract)?;
    validate_digest(expected_digest).map_err(internal_contract)?;
    let obligation = plan
        .obligations
        .iter()
        .find(|ob| ob.task_id == task_id)
        .ok_or_else(|| internal("task_without_obligation"))?;
    if obligation.task_digest != expected_digest {
        return Err(internal("task_digest_mismatch"));
    }
    if !crate::covered_tasks::covered_by_baseline(plan, task_id) {
        let (entry, digest) = super::entry_and_digest(plan, task_id)?;
        if entry.task_id != task_id || entry.task_digest != digest {
            return Err(internal("task_entry_identity_mismatch"));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "task_report_identity_tests.rs"]
mod tests;
