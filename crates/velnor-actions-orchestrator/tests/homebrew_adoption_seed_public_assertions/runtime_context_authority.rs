// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, io};

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_orchestrator::{GenerationPreparation, OrchestratorError, finalized_jobs};
use velnor_actions_workflow_renderer::{RenderError, render::render_workflow_ir};

pub(super) fn assert_runtime_context_authority(
    preparation: &GenerationPreparation,
    policy: WorkflowPolicy,
) -> Result<(), Box<dyn Error>> {
    for key in [
        "BASH_ENV",
        "ENV",
        "LD_PRELOAD",
        "GH_TOKEN",
        "VELNOR_INTERNAL_OP",
    ] {
        let mut changed_env = preparation.clone();
        changed_env
            .workflow
            .context
            .plan_consumer_env
            .insert(key.to_owned(), "candidate-value".to_owned());
        assert_runtime_rejected(&changed_env, policy)?;
    }

    let mut changed_runtime = preparation.clone();
    "9.9.9".clone_into(&mut changed_runtime.workflow.context.report_helper_version);
    "$RUNNER_TEMP/velnor/bin/velnor-actions-9.9.9"
        .clone_into(&mut changed_runtime.workflow.context.staged_binary);
    assert_runtime_rejected(&changed_runtime, policy)?;

    let mut changed_request = preparation.clone();
    "${{ runner.temp }}/velnor/candidate"
        .clone_into(&mut changed_request.workflow.context.request_dir);
    assert_runtime_rejected(&changed_request, policy)?;

    let mut changed_checkout = preparation.clone();
    "actions/checkout@0123456789abcdef0123456789abcdef01234567"
        .clone_into(&mut changed_checkout.workflow.context.checkout_uses);
    assert_runtime_rejected(&changed_checkout, policy)?;

    let mut changed_preseed = preparation.clone();
    changed_preseed.workflow.context.preseed = !changed_preseed.workflow.context.preseed;
    assert_runtime_rejected(&changed_preseed, policy)
}

fn assert_runtime_rejected(
    preparation: &GenerationPreparation,
    policy: WorkflowPolicy,
) -> Result<(), Box<dyn Error>> {
    let workflow = &preparation.workflow;
    let raw_error = render_workflow_ir(
        &workflow.ir,
        policy,
        workflow.support.as_ref(),
        &workflow.context,
    )
    .err()
    .ok_or_else(|| io::Error::other("renderer accepted modified native runtime context"))?;
    assert!(
        matches!(raw_error, RenderError::InvalidWorkflow(problem) if problem == "trusted_plan_runtime_context")
    );
    let strict_error = finalized_jobs(preparation)
        .err()
        .ok_or_else(|| io::Error::other("finalizer accepted modified native runtime context"))?;
    assert!(
        matches!(strict_error, OrchestratorError::Render { problem } if problem == "invalid workflow: trusted_plan_runtime_context")
    );
    Ok(())
}
