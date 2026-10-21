//! Optional `cache-cleanup.yml` emission for pull-request-scoped cache writes.

use velnor_actions_contract::PullRequestCachePolicy;
use velnor_actions_workflow_renderer::render::RenderedFile;

use crate::{OrchestratorError, prepare::GenerationPreparation};

/// Emit the cleanup workflow only under the policy that writes PR-scoped entries.
pub(crate) fn files(prep: &GenerationPreparation) -> Result<Vec<RenderedFile>, OrchestratorError> {
    files_for(
        prep.config.workflow.pull_request_cache_policy,
        &prep.runner_label,
    )
}

fn files_for(
    policy: PullRequestCachePolicy,
    runner_label: &str,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    if policy != PullRequestCachePolicy::SameRepositoryScoped {
        return Ok(Vec::new());
    }
    Ok(vec![
        velnor_actions_workflow_renderer::cache_cleanup::render_cache_cleanup_yml(
            runner_label,
            env!("CARGO_PKG_VERSION"),
        )?,
    ])
}

#[cfg(test)]
#[path = "cache_cleanup_emit_tests.rs"]
mod tests;
