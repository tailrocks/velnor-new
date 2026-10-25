use velnor_actions_contract::PullRequestCachePolicy;
use velnor_actions_workflow_renderer::render::RenderedFile;

use crate::{OrchestratorError, prepare::GenerationPreparation};

#[path = "cache_prune_emit.rs"]
mod prune;

pub(crate) fn files(prep: &GenerationPreparation) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let mut files = files_for(
        prep.config.workflow.pull_request_cache_policy,
        &prep.runner_label,
    )?;
    files.extend(prune::files(prep)?);
    Ok(files)
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
