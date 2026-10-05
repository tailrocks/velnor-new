//! Construction of isolated verification-only task jobs.

use velnor_actions_contract::{VelnorConfig, WorkflowTask};
use velnor_actions_workflow_renderer::VerificationTaskPolicy;

use crate::OrchestratorError;
use crate::pins::resolve_verification_mise_setup;

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
        .filter_map(|task| match task {
            WorkflowTask::Verification(verification) => Some(verification),
            WorkflowTask::Build(_) => None,
        })
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
