//! `cache-prune.yml` emission for repositories that write cache families.

use velnor_actions_workflow_renderer::render::RenderedFile;

use crate::{OrchestratorError, prepare::GenerationPreparation};

/// Emit the prune workflow when the repository has runnable tasks or a stack
/// job that writes a cache family.
pub(crate) fn files(prep: &GenerationPreparation) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let stacks = &prep.config.stacks;
    let writes = !prep.discovery.proposals.is_empty()
        || stacks.bun.is_some()
        || stacks.dbt.is_some()
        || stacks.ansible.is_some();
    files_for(writes, &prep.runner_label, &prep.default_branch)
}

fn files_for(
    writes: bool,
    runner_label: &str,
    default_branch: &str,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    if !writes {
        return Ok(Vec::new());
    }
    Ok(vec![
        velnor_actions_workflow_renderer::cache_prune::render_cache_prune_yml(
            runner_label,
            default_branch,
            env!("CARGO_PKG_VERSION"),
        )?,
    ])
}

#[cfg(test)]
#[path = "cache_prune_emit_tests.rs"]
mod tests;
