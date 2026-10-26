// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{Stack, Step, StepRole, WorkflowPolicy};
use velnor_actions_mise::{
    PREPARE_RUST_COMPONENTS_STEP, PrepareRustComponents, ToolCatalog, ToolHomes,
};
use velnor_actions_rust::TestRunner;

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::utf8::{strings_of, strings_of_env};

#[path = "jactionlint_verify_step.rs"]
pub(crate) mod jactionlint_verify_step;

#[path = "rust_manifest_step.rs"]
pub(crate) mod rust_manifest_step;

pub(super) fn plan_uses_nextest(discovery: &Discovery) -> bool {
    discovery
        .workspaces
        .iter()
        .any(|workspace| workspace.profile.test_runner == TestRunner::CargoNextest)
}

pub(super) fn plan_uses_all_features(discovery: &Discovery) -> bool {
    discovery.proposals.iter().any(|task| {
        task.identity
            .flags
            .iter()
            .any(|flag| flag == "--all-features")
    })
}

pub(crate) fn plan_uses_opentofu(discovery: &Discovery) -> bool {
    discovery
        .proposals
        .iter()
        .any(|task| Stack::from_id(&task.stack_id) == Some(Stack::Tofu))
}

pub(crate) fn plan_uses_rust(discovery: &Discovery, policy: WorkflowPolicy) -> bool {
    policy == WorkflowPolicy::VelnorRepositoryV1
        || !discovery.workspaces.is_empty()
        || discovery.statuses.iter().any(|status| {
            let project = match status {
                velnor_actions_contract::DetectionStatus::Selected(project)
                | velnor_actions_contract::DetectionStatus::Ignored { project, .. } => project,
            };
            Stack::from_id(&project.stack_id) == Some(Stack::Rust)
        })
        || discovery
            .proposals
            .iter()
            .any(|task| Stack::from_id(&task.stack_id) == Some(Stack::Rust))
}

pub(crate) fn prepare_rust_components_step(
    catalog: &ToolCatalog,
) -> Result<Step, OrchestratorError> {
    let request = PrepareRustComponents::new(ToolHomes::runner_temp());
    let run = strings_of(request.argv(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    let env = strings_of_env(&request.env(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    let mut step = velnor_actions_workflow_renderer::ambient_shell_step(
        PREPARE_RUST_COMPONENTS_STEP,
        run,
        env,
    )
    .map_err(OrchestratorError::from)?;
    step.role = Some(StepRole::PrepareRustComponents);
    Ok(step)
}
