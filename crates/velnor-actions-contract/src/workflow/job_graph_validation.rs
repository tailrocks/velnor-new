//! Workflow job dependency graph validation.

use std::collections::{BTreeMap, BTreeSet};

use super::Job;
use crate::errors::ContractError;

/// Reject a directed cycle in the finalized `needs` graph.
pub(super) fn validate(jobs: &BTreeMap<String, Job>) -> Result<(), ContractError> {
    let mut remaining_needs = BTreeMap::new();
    let mut dependents = BTreeMap::<&str, Vec<&str>>::new();
    let mut ready = BTreeSet::new();

    for (job_id, job) in jobs {
        remaining_needs.insert(job_id.as_str(), job.needs.len());
        if job.needs.is_empty() {
            ready.insert(job_id.as_str());
        }
        for dependency in &job.needs {
            dependents
                .entry(dependency.as_str())
                .or_default()
                .push(job_id.as_str());
        }
    }

    let mut visited = 0;
    while let Some(job_id) = ready.pop_first() {
        visited += 1;
        for dependent in dependents.get(job_id).into_iter().flatten() {
            let Some(remaining) = remaining_needs.get_mut(*dependent) else {
                return Err(ContractError::identity(
                    "workflow.jobs",
                    "dependency_graph_invalid",
                ));
            };
            let Some(next) = remaining.checked_sub(1) else {
                return Err(ContractError::identity(
                    "workflow.jobs",
                    "dependency_graph_invalid",
                ));
            };
            *remaining = next;
            if next == 0 {
                ready.insert(dependent);
            }
        }
    }

    if visited == jobs.len() {
        Ok(())
    } else {
        Err(ContractError::identity("workflow.jobs", "dependency_cycle"))
    }
}

#[cfg(test)]
#[path = "job_graph_validation_tests.rs"]
mod tests;
