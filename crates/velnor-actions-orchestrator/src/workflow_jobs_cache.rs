// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::Step;
use velnor_actions_mise::ToolCatalog;

use crate::OrchestratorError;

pub(crate) use velnor_actions_workflow_renderer::plan_sources::PlanCache;

pub(crate) fn cache_steps_for_plan(
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    tofu_roots: &[String],
) -> Result<PlanCache, OrchestratorError> {
    velnor_actions_workflow_renderer::plan_sources::cache_steps_for_plan(
        catalog,
        fetch_roots,
        tofu_roots,
    )
    .map_err(|err| match err {
        velnor_actions_workflow_renderer::plan_sources::PlanSourcesError::Contract(err) => {
            crate::OrchestratorError::from(err)
        }
        velnor_actions_workflow_renderer::plan_sources::PlanSourcesError::ShellRender(err) => {
            crate::OrchestratorError::Contract {
                problem: err.to_string(),
            }
        }
        velnor_actions_workflow_renderer::plan_sources::PlanSourcesError::CacheSourcesPolicy(
            problem,
        ) => crate::OrchestratorError::Contract { problem },
        velnor_actions_workflow_renderer::plan_sources::PlanSourcesError::CacheActionRef(
            problem,
        ) => crate::OrchestratorError::WorkflowLint { problem },
        velnor_actions_workflow_renderer::plan_sources::PlanSourcesError::Render(err) => {
            crate::OrchestratorError::from(err)
        }
    })
}

pub(crate) fn mark_writer_gate(source_steps: &mut [Step]) {
    velnor_actions_workflow_renderer::plan_sources::mark_writer_gate(source_steps);
}
