//! Variant-dispatched workflow-task policies and job insertion.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, VelnorConfig};
use velnor_actions_workflow_renderer::{
    build_verification_task_job,
    verification_jobs::{WorkflowTaskPolicy, build_build_task_job, build_native_image_job},
};

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::workflow::CHECKOUT_USES;

/// Resolve every declared task variant into one sorted policy inventory.
pub(crate) fn policies(
    root: &std::path::Path,
    config: &VelnorConfig,
    discovery: &Discovery,
) -> Result<Vec<WorkflowTaskPolicy>, OrchestratorError> {
    let verification_policies = crate::verification_tasks::policies(config, discovery)?;
    let build_policies = crate::build_tasks::policies(config, discovery)?;
    let native_image_policies = crate::native_image_tasks::policies(root, config)?;
    let mut workflow_tasks = verification_policies
        .iter()
        .cloned()
        .map(WorkflowTaskPolicy::Verification)
        .chain(
            build_policies
                .iter()
                .cloned()
                .map(WorkflowTaskPolicy::Build),
        )
        .chain(
            native_image_policies
                .iter()
                .cloned()
                .map(WorkflowTaskPolicy::NativeImage),
        )
        .collect::<Vec<_>>();
    workflow_tasks.sort_by(|left, right| left.id().cmp(right.id()));
    Ok(workflow_tasks)
}

/// Insert task jobs from the single validated variant-dispatched inventory.
pub(crate) fn insert_jobs(
    jobs: &mut BTreeMap<String, Job>,
    tasks: &[WorkflowTaskPolicy],
) -> Result<(), OrchestratorError> {
    for task in tasks {
        let (id, job) = match task {
            WorkflowTaskPolicy::Verification(policy) => (
                policy.job_id(),
                build_verification_task_job(policy, CHECKOUT_USES)?,
            ),
            WorkflowTaskPolicy::Build(policy) => (
                policy.job_id(),
                build_build_task_job(policy, CHECKOUT_USES)?,
            ),
            WorkflowTaskPolicy::NativeImage(policy) => (
                policy.job_id(),
                build_native_image_job(policy, CHECKOUT_USES)?,
            ),
        };
        if jobs.insert(id.clone(), job).is_some() {
            return Err(OrchestratorError::Contract {
                problem: format!("workflow_task_job_collision:{id}"),
            });
        }
    }
    Ok(())
}
