//! Workflow policy predicates and pinned tool preparation.

use velnor_actions_contract::{Stack, Step, StepRole, WorkflowPolicy};
use velnor_actions_mise::{
    PREPARE_RUST_COMPONENTS_STEP, PrepareRustComponents, ToolCatalog, ToolHomes,
};
use velnor_actions_rust::TestRunner;

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::utf8::{strings_of, strings_of_env};

/// True when any selected workspace runs tests through Nextest; only those
/// legs resolve the pinned runner.
pub(super) fn plan_uses_nextest(discovery: &Discovery) -> bool {
    discovery
        .workspaces
        .iter()
        .any(|workspace| workspace.profile.test_runner == TestRunner::CargoNextest)
}

/// True when any proposal runs through the pinned Opentofu driver.
///
/// Tofu has no workspace profiles, so the plan derives its tofu role
/// from task proposals (the same per-task signal crate jobs group
/// on), never from workspace scans.
pub(crate) fn plan_uses_opentofu(discovery: &Discovery) -> bool {
    discovery
        .proposals
        .iter()
        .any(|task| Stack::from_id(&task.stack_id) == Some(Stack::Tofu))
}

/// True when the plan job needs the Rust toolchain.
///
/// Consumers require Rust for selected Rust evidence (selected or ignored
/// Rust projects, or Rust task proposals without inventory records).
/// Velnor also builds its candidate-source helper in Plan.
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

/// Typed `Prepare Rust components` step, shared by plan and task jobs.
///
/// Runs second, right after `Prepare pinned tools`: the pinned toolchain
/// exists by then, so the fixed `rustup component add` guarantees
/// clippy/rustfmt idempotently under the owned homes.
///
/// # Errors
///
/// Returns a contract error when the Mise adapter rejects the request.
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
