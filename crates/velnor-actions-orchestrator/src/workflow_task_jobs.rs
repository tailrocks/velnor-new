//! Variant-dispatched workflow-task policies and job insertion.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, VelnorConfig};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::{
    build_verification_task_job,
    verification_jobs::{
        WorkflowTaskPolicy, build_build_task_job, build_image_build_job, build_native_image_job,
    },
};

use crate::OrchestratorError;
use crate::config::private_wiring::PrivateWiring;
use crate::discover::Discovery;
use crate::workflow::CHECKOUT_USES;

/// Resolve every declared task variant into one sorted policy inventory.
pub(crate) fn policies(
    root: &std::path::Path,
    config: &VelnorConfig,
    discovery: &Discovery,
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    acquire: Option<&Step>,
    private: Option<&PrivateWiring>,
) -> Result<Vec<WorkflowTaskPolicy>, OrchestratorError> {
    let verification_policies = crate::verification_tasks::policies(config, discovery)?;
    let build_policies = crate::build_tasks::policies(config, discovery)?;
    let native_image_policies = crate::native_image_tasks::policies(root, config)?;
    let image_policies = crate::image_build_tasks::policies(
        root,
        config,
        &crate::image_build_tasks::PrebuiltInputs {
            discovery,
            catalog,
            fetch_roots,
            acquire,
            private,
        },
    )?;
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
        .chain(image_policies.into_iter().map(WorkflowTaskPolicy::Image))
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
        let built = match task {
            WorkflowTaskPolicy::Verification(policy) => vec![(
                policy.job_id(),
                build_verification_task_job(policy, CHECKOUT_USES)?,
            )],
            WorkflowTaskPolicy::Build(policy) => vec![(
                policy.job_id(),
                build_build_task_job(policy, CHECKOUT_USES)?,
            )],
            WorkflowTaskPolicy::NativeImage(policy) => vec![(
                policy.job_id(),
                build_native_image_job(policy, CHECKOUT_USES)?,
            )],
            WorkflowTaskPolicy::Image(policy) => policy
                .image
                .platforms
                .iter()
                .map(|platform| {
                    Ok((
                        policy.image.build_job_id(*platform),
                        build_image_build_job(policy, *platform, CHECKOUT_USES)?,
                    ))
                })
                .collect::<Result<Vec<_>, OrchestratorError>>()?,
        };
        for (id, job) in built {
            if jobs.insert(id.clone(), job).is_some() {
                return Err(OrchestratorError::Contract {
                    problem: format!("workflow_task_job_collision:{id}"),
                });
            }
        }
    }
    Ok(())
}
