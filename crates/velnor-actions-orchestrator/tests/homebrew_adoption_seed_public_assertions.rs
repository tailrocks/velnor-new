// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_orchestrator::GenerationPreparation;

mod complete_job_authority;
mod helper_tools;
mod lock_trust;
mod native_context_authority;
mod native_fact_inventory;
mod raw_step_authority;
mod required_steps;
mod runtime_context_authority;
mod shell_authority;

pub(super) fn assert_raw_consumer_render(
    preparation: &GenerationPreparation,
) -> Result<(), Box<dyn Error>> {
    assert_raw_render(preparation, WorkflowPolicy::ConsumerV1)?;
    runtime_context_authority::assert_runtime_context_authority(
        preparation,
        WorkflowPolicy::ConsumerV1,
    )?;
    shell_authority::assert_shell_authority(preparation, WorkflowPolicy::ConsumerV1)?;
    raw_step_authority::assert_raw_step_authority(preparation, WorkflowPolicy::ConsumerV1)
}

pub(super) fn assert_seed_pull_request_events(yaml: &str) {
    let pull_request_types = yaml
        .lines()
        .skip_while(|line| *line != "  pull_request:")
        .skip(1)
        .take_while(|line| line.starts_with("    "))
        .filter_map(|line| line.trim().strip_prefix("- "))
        .collect::<Vec<_>>();
    assert_eq!(
        pull_request_types,
        [
            "opened",
            "edited",
            "synchronize",
            "reopened",
            "ready_for_review"
        ],
        "seed must re-plan pull requests after base retargets and when drafts become ready"
    );
}

fn assert_raw_render(
    preparation: &GenerationPreparation,
    policy: WorkflowPolicy,
) -> Result<(), Box<dyn Error>> {
    let yaml = velnor_actions_workflow_renderer::render::render_workflow_ir(
        &preparation.workflow.ir,
        policy,
        preparation.workflow.support.as_ref(),
        &preparation.workflow.context,
    )?;
    assert!(!yaml.contains("jdx/mise-action@"));
    Ok(())
}
