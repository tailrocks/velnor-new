//! Variant-dispatched workflow-task policies and job insertion.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, VelnorConfig};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::{
    build_verification_task_job,
    verification_jobs::{
        WorkflowTaskPolicy, build_build_task_job, build_bun_member_job, build_bun_suite_job,
        build_image_build_job, build_native_image_job, build_stack_job,
    },
};

use crate::OrchestratorError;
use crate::config::private_wiring::PrivateWiring;
use crate::discover::Discovery;
use crate::workflow::CHECKOUT_USES;

#[path = "ansible_inventory.rs"]
pub(crate) mod ansible_inventory;
#[path = "ansible_steps.rs"]
mod ansible_steps;
#[path = "ansible_tasks.rs"]
mod ansible_tasks;
#[path = "bun_inventory.rs"]
pub(crate) mod bun_inventory;
#[path = "bun_steps.rs"]
pub(crate) mod bun_steps;
#[path = "bun_tasks.rs"]
mod bun_tasks;
#[path = "dbt_inventory.rs"]
pub(crate) mod dbt_inventory;
#[path = "dbt_steps.rs"]
mod dbt_steps;
#[path = "dbt_tasks.rs"]
mod dbt_tasks;
#[path = "stack_family_cache.rs"]
pub(crate) mod stack_family_cache;
#[path = "stack_support.rs"]
pub(crate) mod stack_support;

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
    let bun_policies = bun_tasks::policies(root, config, discovery, catalog, acquire, private)?;
    let dbt_policies = dbt_tasks::policies(config, discovery, catalog)?;
    let ansible_policies = ansible_tasks::policies(config, discovery, catalog)?;
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
        .chain(
            bun_policies
                .members
                .into_iter()
                .map(WorkflowTaskPolicy::Bun),
        )
        .chain(
            bun_policies
                .suites
                .into_iter()
                .map(WorkflowTaskPolicy::BunSuite),
        )
        .chain(dbt_policies.into_iter().map(WorkflowTaskPolicy::Stack))
        .chain(ansible_policies.into_iter().map(WorkflowTaskPolicy::Stack))
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
            WorkflowTaskPolicy::Bun(policy) => vec![(
                policy.id.clone(),
                build_bun_member_job(policy, CHECKOUT_USES)?,
            )],
            WorkflowTaskPolicy::BunSuite(policy) => vec![(
                policy.id.clone(),
                build_bun_suite_job(policy, CHECKOUT_USES)?,
            )],
            WorkflowTaskPolicy::Stack(policy) => {
                vec![(policy.id.clone(), build_stack_job(policy, CHECKOUT_USES)?)]
            }
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
