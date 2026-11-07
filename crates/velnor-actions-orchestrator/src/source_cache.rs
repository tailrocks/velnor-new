// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::Step;
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::plan_sources::PlanSourcesError;

use crate::OrchestratorError;

fn orchestrator_error(err: PlanSourcesError) -> OrchestratorError {
    match err {
        PlanSourcesError::Contract(err) => OrchestratorError::from(err),
        PlanSourcesError::CacheSourcesPolicy(problem) => OrchestratorError::Contract { problem },
        PlanSourcesError::CacheActionRef(problem) => OrchestratorError::WorkflowLint { problem },
        PlanSourcesError::Render(err) | PlanSourcesError::ShellRender(err) => {
            OrchestratorError::from(err)
        }
    }
}

pub(crate) fn restore_step_for_crate(
    fetch_roots: &[String],
    use_rust: bool,
) -> Result<Option<Step>, OrchestratorError> {
    velnor_actions_workflow_renderer::plan_sources::source_cache::restore_step_for_crate(
        fetch_roots,
        use_rust,
    )
    .map_err(orchestrator_error)
}

pub(crate) fn keyed_restore_steps(
    catalog: &ToolCatalog,
    fetch_roots: &[String],
) -> Result<Vec<Step>, OrchestratorError> {
    velnor_actions_workflow_renderer::plan_sources::cache_steps_for_plan(
        catalog,
        fetch_roots,
        &[],
        &[],
    )
    .map_err(orchestrator_error)
}
