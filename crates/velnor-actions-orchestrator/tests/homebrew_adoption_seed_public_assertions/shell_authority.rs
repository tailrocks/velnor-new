// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, io};

use velnor_actions_contract::{StepKind, WorkflowPolicy};
use velnor_actions_orchestrator::{GenerationPreparation, OrchestratorError, finalized_jobs};
use velnor_actions_workflow_renderer::{
    RenderError,
    render::{FINAL_JOB_ID, PLAN_JOB_ID, render_workflow_ir},
};

pub(super) fn assert_shell_authority(
    preparation: &GenerationPreparation,
    policy: WorkflowPolicy,
) -> Result<(), Box<dyn Error>> {
    let ids = if policy == WorkflowPolicy::VelnorRepositoryV1 {
        vec![FINAL_JOB_ID, PLAN_JOB_ID]
    } else {
        vec![FINAL_JOB_ID]
    };
    for id in ids {
        let job = preparation
            .workflow
            .ir
            .jobs
            .get(id)
            .ok_or_else(|| io::Error::other(format!("workflow omitted {id}")))?;
        let indices = job
            .steps
            .iter()
            .enumerate()
            .filter_map(|(index, step)| {
                matches!(&step.kind, StepKind::Shell { .. }).then_some(index)
            })
            .collect::<Vec<_>>();
        assert!(!indices.is_empty(), "{id} must exercise actual shell steps");
        for index in indices {
            for environment in [false, true] {
                let mut changed = preparation.clone();
                let step = changed
                    .workflow
                    .ir
                    .jobs
                    .get_mut(id)
                    .and_then(|job| job.steps.get_mut(index))
                    .ok_or("authority step disappeared")?;
                let StepKind::Shell { run, env } = &mut step.kind else {
                    return Err("authority shell changed kind".into());
                };
                if environment {
                    env.insert("BASH_ENV".to_owned(), "candidate-hook.sh".to_owned());
                } else {
                    *run = vec!["bash".to_owned(), "-c".to_owned(), "exit 23".to_owned()];
                }
                let expected = if id == FINAL_JOB_ID {
                    "required_verdict_candidate_execution"
                } else {
                    "trusted_plan_candidate_execution"
                };
                let raw = render_workflow_ir(
                    &changed.workflow.ir,
                    policy,
                    changed.workflow.support.as_ref(),
                    &changed.workflow.context,
                )
                .err()
                .ok_or("raw rendering accepted candidate-controlled authority shell")?;
                assert!(
                    matches!(raw, RenderError::InvalidWorkflow(problem) if problem == expected)
                );
                let strict = finalized_jobs(&changed)
                    .err()
                    .ok_or("finalization accepted candidate-controlled authority shell")?;
                assert!(
                    matches!(&strict, OrchestratorError::Render { problem }
                    if problem == &format!("invalid workflow: {expected}")),
                    "unexpected strict shell authority error: {strict}"
                );
            }
        }
    }
    Ok(())
}
