//! Preserve task gates between package jobs.

use std::collections::BTreeSet;

use velnor_actions_contract::ProposedTask;
use velnor_actions_contract::WorkflowEvent;
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;

use crate::OrchestratorError;
use crate::crate_job_ids::JobOwners;
use crate::internal::internal;

/// Get the plan job and every external owner for these tasks.
pub(super) fn external_needs(
    tasks: &[&ProposedTask],
    owners: &JobOwners<'_>,
    current_job: &str,
    event: WorkflowEvent,
) -> Result<Vec<String>, OrchestratorError> {
    let mut needs = BTreeSet::from([PLAN_JOB_ID.to_owned()]);
    for task in tasks {
        for gate in task.gated_by.iter().chain(&task.depends_on) {
            let Some(member) = owners.runnable_member(gate) else {
                continue;
            };
            let owner = owners
                .for_member(member, event)?
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
