// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, VelnorConfig};
use velnor_actions_workflow_renderer::{
    build_verification_task_job,
    verification_jobs::{
        WorkflowTaskPolicy, build_build_task_job, build_bun_assets_job, build_bun_member_job,
        build_bun_report_job, build_bun_suite_job, build_image_build_job, build_native_image_job,
        build_stack_job,
    },
};

use crate::OrchestratorError;
use crate::workflow::CHECKOUT_USES;

#[path = "ansible_inventory.rs"]
pub(crate) mod ansible_inventory;
#[path = "ansible_steps.rs"]
mod ansible_steps;
#[path = "ansible_tasks.rs"]
mod ansible_tasks;
#[path = "bun_assets_steps.rs"]
pub(crate) mod bun_assets_steps;
#[path = "bun_inventory.rs"]
pub(crate) mod bun_inventory;
#[path = "bun_report_steps.rs"]
pub(crate) mod bun_report_steps;
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

pub(crate) fn policies(
    root: &std::path::Path,
    config: &VelnorConfig,
    inputs: &crate::image_build_tasks::PrebuiltInputs<'_>,
) -> Result<Vec<WorkflowTaskPolicy>, OrchestratorError> {
    let crate::image_build_tasks::PrebuiltInputs {
        discovery,
        catalog,
        acquire,
        private,
        ..
    } = *inputs;
    let verification_policies = crate::verification_tasks::policies(config, discovery)?;
    let build_policies = crate::build_tasks::policies(config, discovery)?;
    let native_image_policies = crate::native_image_tasks::policies(root, config)?;
    let image_policies = crate::image_build_tasks::policies(root, config, inputs)?;
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
        .chain(
            bun_policies
                .assets
                .into_iter()
                .map(WorkflowTaskPolicy::BunAssets),
        )
        .chain(dbt_policies.into_iter().map(WorkflowTaskPolicy::Stack))
        .chain(ansible_policies.into_iter().map(WorkflowTaskPolicy::Stack))
        .collect::<Vec<_>>();
    if let Some(policy) = runner_family_policy(root, config, discovery)? {
        workflow_tasks.push(policy);
    }
    workflow_tasks.sort_by(|left, right| left.id().cmp(right.id()));
    Ok(workflow_tasks)
}

fn runner_family_policy(
    root: &std::path::Path,
    config: &VelnorConfig,
    discovery: &crate::discover::Discovery,
) -> Result<Option<WorkflowTaskPolicy>, OrchestratorError> {
    if config.runner_image_family.is_none() {
        return Ok(None);
    }
    let bytes = match crate::safe_read::read_repo_bytes(
        root,
        "scripts/check-runner-image-candidate.sh",
        1024 * 1024,
    )? {
        crate::safe_read::RepoBytes::Bytes(bytes) => bytes,
        crate::safe_read::RepoBytes::Absent => {
            return Err(OrchestratorError::Contract {
                problem: "runner_family_script_missing".into(),
            });
        }
    };
    let (tools, mise_config, mise_lock, rust_toolchain) =
        crate::build_tasks::runner_family_tools(discovery)?;
    let mut prefetch_sha256 = std::collections::BTreeMap::new();
    for path in velnor_actions_workflow_renderer::verification_jobs::FAMILY_PREFETCH_SOURCES {
        let bytes = match crate::safe_read::read_repo_bytes(root, path, 1024 * 1024)? {
            crate::safe_read::RepoBytes::Bytes(bytes) => bytes,
            crate::safe_read::RepoBytes::Absent => {
                return Err(OrchestratorError::Contract {
                    problem: "runner_family_prefetch_source_missing".into(),
                });
            }
        };
        prefetch_sha256.insert(path.to_owned(), crate::generator::sha256_hex(&bytes));
    }
    let task = velnor_actions_contract::VerificationTask {
        id: "runner-image-family".into(),
        mise_task: "check-runner-image-candidate".into(),
        source: velnor_actions_contract::MiseTaskSource {
            mise_config: "mise.toml".into(),
            working_directory: ".".into(),
        },
        runner: velnor_actions_contract::VerificationRunner::LinuxX64,
        timeout_minutes: 120,
    };
    let bootstrap = velnor_actions_workflow_renderer::VerificationTaskPolicy {
        task,
        runner_label: "ubuntu-26.04".into(),
        scale_set_token: None,
        mise_setup: crate::pins::resolve_mise_setup(config, "ubuntu-26.04")?,
        selected_tools: tools,
        mise_config_sha256: Some(mise_config),
        mise_lock_sha256: Some(mise_lock),
        rust_toolchain_sha256: Some(rust_toolchain),
    };
    Ok(Some(WorkflowTaskPolicy::RunnerFamily(
        velnor_actions_workflow_renderer::verification_jobs::RunnerFamilyPolicy {
            bootstrap,
            script_sha256: crate::generator::sha256_hex(&bytes),
            prefetch_sha256,
        },
    )))
}

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
            WorkflowTaskPolicy::RunnerFamily(policy) => vec![(
                velnor_actions_contract::config::RUNNER_IMAGE_FAMILY_JOB_ID.to_owned(),
                velnor_actions_workflow_renderer::verification_jobs::build_runner_family_job(
                    policy,
                    CHECKOUT_USES,
                )?,
            )],
            WorkflowTaskPolicy::Bun(policy) => vec![(
                policy.id.clone(),
                build_bun_member_job(policy, CHECKOUT_USES)?,
            )],
            WorkflowTaskPolicy::BunAssets(policy) => vec![(
                policy.id.clone(),
                build_bun_assets_job(policy, CHECKOUT_USES)?,
            )],
            WorkflowTaskPolicy::BunSuite(policy) => {
                let mut built = vec![(
                    policy.id.clone(),
                    build_bun_suite_job(policy, CHECKOUT_USES)?,
                )];
                if policy.report.is_some() {
                    built.push((
                        policy.report_id(),
                        build_bun_report_job(policy, CHECKOUT_USES)?,
                    ));
                }
                built
            }
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
