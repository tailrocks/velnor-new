//! Preserve task gates between package jobs.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::ProposedTask;
use velnor_actions_contract::WorkflowEvent;
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;

use crate::OrchestratorError;
use crate::crate_job_ids::job_id_for_member;
use crate::internal::internal;

/// Get the plan job and every external owner for these tasks.
pub(super) fn external_needs(
    tasks: &[&ProposedTask],
    proposals: &[ProposedTask],
    current_job: &str,
    event: WorkflowEvent,
) -> Result<Vec<String>, OrchestratorError> {
    let runnable: BTreeMap<&str, &ProposedTask> = proposals
        .iter()
        .filter(|task| super::is_runnable(task))
        .map(|task| (task.task_id.as_str(), task))
        .collect();
    let mut needs = BTreeSet::from([PLAN_JOB_ID.to_owned()]);
    for task in tasks {
        for gate in task.gated_by.iter().chain(&task.depends_on) {
            let Some(member) = runnable.get(gate.as_str()) else {
                continue;
            };
            let owner = job_id_for_member(proposals, member, event)?
                .ok_or_else(|| internal("crate_gate_owner_missing"))?;
            if owner != current_job {
                needs.insert(owner);
            }
        }
    }
    Ok(needs.into_iter().collect())
}

#[cfg(test)]
#[path = "crate_jobs_dependencies_tests.rs"]
mod tests;
