// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_workflow_renderer::render::RenderedFile;

use crate::{OrchestratorError, prepare::GenerationPreparation};

pub(crate) fn files(prep: &GenerationPreparation) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let stacks = &prep.config.stacks;
    let writes = !prep.discovery.proposals.is_empty()
        || stacks.bun.is_some()
        || stacks.dbt.is_some()
        || stacks.ansible.is_some();
    let placed = match prep.config.execution.as_ref() {
        Some(execution)
            if velnor_actions_contract::workflow::scale_set_control_jobs(&prep.config)
                .contains("prune") =>
        {
            let selector = execution.scale_selector().map_err(|error| {
                OrchestratorError::config("velnor.toml", "execution", error.to_string())
            })?;
            Some(selector.token())
        }
        _ => None,
    };
    files_for(
        writes,
        placed.as_deref(),
        &prep.runner_label,
        &prep.default_branch,
        &prep.config.workflow.name,
    )
}

fn files_for(
    writes: bool,
    scale_set: Option<&str>,
    runner_label: &str,
    default_branch: &str,
    workflow_name: &str,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    if !writes {
        return Ok(Vec::new());
    }
    Ok(vec![
        velnor_actions_workflow_renderer::cache_prune::render_cache_prune_yml_placed(
            runner_label,
            scale_set,
            default_branch,
            workflow_name,
            env!("CARGO_PKG_VERSION"),
        )?,
    ])
}
