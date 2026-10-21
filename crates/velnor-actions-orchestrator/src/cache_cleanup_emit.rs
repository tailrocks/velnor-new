//! Optional `cache-cleanup.yml` emission for pull-request-scoped cache writes.

use velnor_actions_contract::config::ValidatedImageReference;
use velnor_actions_contract::{PullRequestCachePolicy, ScaleSetSelector};
use velnor_actions_workflow_renderer::{WorkflowRoutingPolicy, render::RenderedFile};

use crate::{OrchestratorError, prepare::GenerationPreparation};

/// Emit the cleanup workflow only under the policy that writes PR-scoped entries.
pub(crate) fn files(
    prep: &GenerationPreparation,
    routing: &WorkflowRoutingPolicy,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let selected = if routing.is_scale_set() {
        let execution =
            prep.config
                .execution
                .as_ref()
                .ok_or_else(|| OrchestratorError::Contract {
                    problem: "cache_cleanup_scale_set_execution_missing".to_owned(),
                })?;
        let selector = execution
            .scale_selector()
            .map_err(|error| OrchestratorError::Contract {
                problem: error.to_string(),
            })?;
        let image = execution
            .profiles
            .get(&execution.scale_set_profile)
            .and_then(|profile| profile.job_container_image.as_ref())
            .ok_or_else(|| OrchestratorError::Contract {
                problem: "cache_cleanup_scale_set_container_missing".to_owned(),
            })?;
        Some((selector, image.clone()))
    } else {
        None
    };
    let target = selected.as_ref().map(|(selector, image)| (selector, image));
    files_for(
        prep.config.workflow.pull_request_cache_policy,
        &prep.runner_label,
        routing,
        target,
    )
}

fn files_for(
    policy: PullRequestCachePolicy,
    runner_label: &str,
    routing: &WorkflowRoutingPolicy,
    selected: Option<(&ScaleSetSelector, &ValidatedImageReference)>,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    if policy != PullRequestCachePolicy::SameRepositoryScoped {
        return Ok(Vec::new());
    }
    let (runs_on, image) = match (routing.is_scale_set(), selected) {
        (false, None) => (runner_label.to_owned(), None),
        (true, Some((selector, image))) => (selector.token(), Some(image)),
        (true, None) => {
            return Err(OrchestratorError::Contract {
                problem: "cache_cleanup_scale_set_route_missing".to_owned(),
            });
        }
        (false, Some(_)) => {
            return Err(OrchestratorError::Contract {
                problem: "cache_cleanup_hosted_route_has_scale_set_target".to_owned(),
            });
        }
    };
    Ok(vec![
        velnor_actions_workflow_renderer::cache_cleanup::render_cache_cleanup_yml(
            &runs_on,
            image,
            env!("CARGO_PKG_VERSION"),
        )?,
    ])
}

#[cfg(test)]
#[path = "cache_cleanup_emit_tests.rs"]
mod tests;
