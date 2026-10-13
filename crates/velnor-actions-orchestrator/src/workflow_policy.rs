//! Workflow policy predicates and pinned tool preparation.

use velnor_actions_contract::{Stack, WorkflowPolicy};
use velnor_actions_rust::TestRunner;

use crate::discover::Discovery;

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
