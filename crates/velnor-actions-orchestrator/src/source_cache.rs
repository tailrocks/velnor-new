// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::Step;

pub(crate) fn restore_step_for_crate(
    fetch_roots: &[String],
    use_rust: bool,
) -> Result<Option<Step>, crate::OrchestratorError> {
    velnor_actions_workflow_renderer::plan_sources::source_cache::restore_step_for_crate(
        fetch_roots,
        use_rust,
    )
    .map_err(|err| match err {
        velnor_actions_workflow_renderer::plan_sources::PlanSourcesError::Contract(err) => {
            crate::OrchestratorError::from(err)
        }
        velnor_actions_workflow_renderer::plan_sources::PlanSourcesError::CacheSourcesPolicy(
            problem,
        ) => crate::OrchestratorError::Contract { problem },
        velnor_actions_workflow_renderer::plan_sources::PlanSourcesError::CacheActionRef(
            problem,
        ) => crate::OrchestratorError::WorkflowLint { problem },
        velnor_actions_workflow_renderer::plan_sources::PlanSourcesError::Render(err)
        | velnor_actions_workflow_renderer::plan_sources::PlanSourcesError::ShellRender(err) => {
            crate::OrchestratorError::from(err)
        }
    })
}
