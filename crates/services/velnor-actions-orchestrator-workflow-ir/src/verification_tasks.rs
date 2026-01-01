//! Construction of isolated verification-only task jobs.

use std::collections::BTreeMap;

use velnor_actions_contract_config::{VelnorConfig, WorkflowPolicy};
use velnor_actions_contract_workflow::Job;
use velnor_actions_workflow_jobs::{VerificationTaskPolicy, build_verification_task_job};

use crate::workflow::CHECKOUT_USES;
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_discovery::discover::Discovery;
use velnor_actions_orchestrator_pins::pins::consumer_acquire_step;
use velnor_actions_orchestrator_pins::pins::resolve_verification_mise_setup;

/// Resolve each workflow task onto its fixed platform and Mise binary pin.
pub(crate) fn policies(
    config: &VelnorConfig,
    workflow_policy: WorkflowPolicy,
    version: &str,
    discovery: &Discovery,
) -> Result<Vec<VerificationTaskPolicy>, OrchestratorError> {
    let scale_set_token = match &config.execution {
        Some(execution) => Some(
            execution
                .scale_selector()
                .map_err(|error| OrchestratorError::Contract {
                    problem: error.to_string(),
                })?
                .token(),
        ),
        None => None,
    };
    config
        .workflow
        .tasks
        .iter()
        .map(|task| {
            let staging_steps = if task.outputs.is_empty()
                || workflow_policy == WorkflowPolicy::VelnorRepositoryV1
            {
                Vec::new()
            } else {
                vec![consumer_acquire_step(
                    task.runner.runs_on(),
                    version,
                    discovery,
                )?]
            };
            Ok(VerificationTaskPolicy {
                task: task.clone(),
                runner_label: task.runner.runs_on().to_owned(),
                scale_set_token: scale_set_token.clone(),
                mise_setup: resolve_verification_mise_setup(config, task.runner)?,
                staging_steps,
            })
        })
        .collect()
}

/// Insert exact isolated jobs and reject ID collisions with generator jobs.
pub(crate) fn insert_jobs(
    jobs: &mut BTreeMap<String, Job>,
    policies: &[VerificationTaskPolicy],
) -> Result<(), OrchestratorError> {
    for policy in policies {
        let id = policy.job_id();
        if jobs.contains_key(&id) {
            return Err(OrchestratorError::Contract {
                problem: format!("verification_job_collision:{id}"),
            });
        }
        let job = build_verification_task_job(policy, CHECKOUT_USES)?;
        jobs.insert(id, job);
    }
    Ok(())
}
