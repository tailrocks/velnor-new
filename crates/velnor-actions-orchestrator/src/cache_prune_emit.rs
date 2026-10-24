//! Optional default-branch cache-pruning workflow emission.

use velnor_actions_contract::ScaleSetSelector;
use velnor_actions_contract::config::ValidatedImageReference;
use velnor_actions_workflow_renderer::{WorkflowRoutingPolicy, render::RenderedFile};

use crate::{OrchestratorError, prepare::GenerationPreparation};

/// Emit pruning when a generated job can write one of the cache families.
pub(crate) fn files(
    prep: &GenerationPreparation,
    routing: &WorkflowRoutingPolicy,
    selected: Option<(&ScaleSetSelector, Option<&ValidatedImageReference>)>,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    files_for(
        has_cache_writers(prep),
        &prep.runner_label,
        &prep.default_branch,
        routing,
        selected,
    )
}

fn files_for(
    writes: bool,
    runner_label: &str,
    default_branch: &str,
    routing: &WorkflowRoutingPolicy,
    selected: Option<(&ScaleSetSelector, Option<&ValidatedImageReference>)>,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    if !writes {
        return Ok(Vec::new());
    }
    let (runs_on, image) = super::route(runner_label, routing, selected)?;
    Ok(vec![
        velnor_actions_workflow_renderer::cache_prune::render_cache_prune_yml(
            &runs_on,
            image,
            default_branch,
            env!("CARGO_PKG_VERSION"),
        )?,
    ])
}

fn has_cache_writers(prep: &GenerationPreparation) -> bool {
    !prep.discovery.proposals.is_empty()
        || prep.config.stacks.bun.is_some()
        || prep.config.stacks.dbt.is_some()
        || prep.config.stacks.ansible.is_some()
}

#[cfg(test)]
#[path = "cache_prune_emit_tests.rs"]
mod tests;
