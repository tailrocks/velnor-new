use velnor_actions_contract::WorkflowIr;
use velnor_actions_contract::workflow::{StepKind, StepRole};
use velnor_actions_workflow_renderer::verification_jobs::WorkflowTaskPolicy;

use super::{VerifierActionSet, call_step, matches_canonical_shell_step};
use crate::OrchestratorError;

pub(super) fn lower(
    actions: &VerifierActionSet,
    workflow: &mut WorkflowIr,
    workflow_tasks: &[WorkflowTaskPolicy],
) -> Result<(), OrchestratorError> {
    for (job_id, job) in &mut workflow.jobs {
        for step in &mut job.steps {
            let StepKind::Shell { .. } = &step.kind else {
                continue;
            };
            if !matches!(
                step.role,
                Some(StepRole::VerifyRustManifest | StepRole::VerifyJactionlint)
            ) {
                continue;
            }

            let matching = [
                &actions.restored_rust,
                &actions.installed_rust,
                &actions.jactionlint,
            ]
            .into_iter()
            .filter(|action| action.matches_shell_step(step))
            .collect::<Vec<_>>();
            let Some(action) = matching.first() else {
                if actions
                    .inline_source_steps
                    .iter()
                    .any(|candidate| matches_canonical_shell_step(step, candidate))
                {
                    continue;
                }
                return Err(OrchestratorError::Contract {
                    problem: "noncanonical_generated_verifier_shell_step".to_owned(),
                });
            };
            if matching.len() > 1 {
                return Err(OrchestratorError::Contract {
                    problem: "ambiguous_generated_verifier_shell_step".to_owned(),
                });
            }

            // Image policies reconstruct their prebuilt verifier shells and
            // compare the complete job, so their typed-owned calls stay inline.
            if workflow_tasks.iter().any(|task| {
                matches!(task, WorkflowTaskPolicy::Image(policy) if policy.platform_for_job(job_id).is_some())
            }) {
                continue;
            }

            let display_name = step.name.clone();
            *step = call_step(&display_name, action.uses, Some(action.role));
        }
    }
    Ok(())
}
