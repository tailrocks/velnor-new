//! Resolve a task's unique plan entry and obligation digest.

use velnor_actions_contract::{MatrixEntry, Plan, validate_task_id};

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};

use super::flattened;

/// Resolve one unambiguous task entry and its plan-bound digest.
///
/// The entry must name exactly this task: multi-task (sharded-ref) entries
/// have no per-task outcome here and refuse rather than emit a lying aggregate.
/// No producer emits sharded refs today.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for unknown tasks, tasks without an
/// obligation digest, and multi-task entries.
pub(crate) fn entry_and_digest<'a>(
    plan: &'a Plan,
    task_id: &str,
) -> Result<(&'a MatrixEntry, &'a str), OrchestratorError> {
    entry_and_digest_filtered(plan, task_id, None)
}

/// Resolve one named-check report entry by its generated job identity.
pub(crate) fn entry_and_digest_for_job<'a>(
    plan: &'a Plan,
    task_id: &str,
    job_id: &str,
) -> Result<(&'a MatrixEntry, &'a str), OrchestratorError> {
    entry_and_digest_filtered(plan, task_id, Some(job_id))
}

fn entry_and_digest_filtered<'a>(
    plan: &'a Plan,
    task_id: &str,
    job_id: Option<&str>,
) -> Result<(&'a MatrixEntry, &'a str), OrchestratorError> {
    validate_task_id(task_id).map_err(internal_contract)?;
    let mut found = None;
    for entry in &plan.matrix.include {
        if job_id.is_some_and(|job_id| entry.job_id != job_id) {
            continue;
        }
        let task_ids = flattened(entry);
        if !task_ids.contains(&task_id) {
            continue;
        }
        if task_ids.len() != 1 {
            return Err(internal("multi_task_entry"));
        }
        if found.is_some() {
            return Err(internal("duplicate_task_entry"));
        }
        found = Some(entry);
    }
    let entry = found.ok_or_else(|| match job_id {
        Some(_) => internal("task_not_in_plan_for_job"),
        None => internal("task_not_in_plan"),
    })?;
    let digest = plan
        .obligations
        .iter()
        .find(|obligation| obligation.task_id == task_id)
        .map(|obligation| obligation.task_digest.as_str())
        .ok_or_else(|| internal("task_without_obligation"))?;
    Ok((entry, digest))
}
