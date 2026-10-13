//! Construction of isolated verification-only task jobs.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, VelnorConfig};
use velnor_actions_workflow_renderer::{VerificationTaskPolicy, build_verification_task_job};

use crate::OrchestratorError;
use crate::pins::resolve_verification_mise_setup;
use crate::workflow::CHECKOUT_USES;

/// Resolve each workflow task onto its fixed platform and Mise binary pin.
pub(crate) fn policies(
    config: &VelnorConfig,
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
            Ok(VerificationTaskPolicy {
                task: task.clone(),
                runner_label: task.runner.runs_on().to_owned(),
                scale_set_token: scale_set_token.clone(),
                mise_setup: resolve_verification_mise_setup(config, task.runner)?,
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
        jobs.insert(id, build_verification_task_job(policy, CHECKOUT_USES)?);
    }
    Ok(())
}
