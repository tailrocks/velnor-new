//! Emit generated workflows that remove superseded cache entries.

use velnor_actions_contract::config::ValidatedImageReference;
use velnor_actions_contract::{PullRequestCachePolicy, ScaleSetSelector};
use velnor_actions_workflow_renderer::{WorkflowRoutingPolicy, render::RenderedFile};

use crate::{OrchestratorError, prepare::GenerationPreparation};

#[path = "cache_prune_emit.rs"]
mod prune;

/// Emit cache cleanup and pruning workflows for the selected runner route.
pub(crate) fn files(
    prep: &GenerationPreparation,
    routing: &WorkflowRoutingPolicy,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let selected = selected_scale_set_target(prep, routing)?;
    let target = selected
        .as_ref()
        .map(|(selector, image)| (selector, image.as_ref()));
    let mut files = files_for(
        prep.config.workflow.pull_request_cache_policy,
        &prep.runner_label,
        routing,
        target,
    )?;
    files.extend(prune::files(prep, routing, target)?);
    Ok(files)
}

fn selected_scale_set_target(
    prep: &GenerationPreparation,
    routing: &WorkflowRoutingPolicy,
) -> Result<Option<(ScaleSetSelector, Option<ValidatedImageReference>)>, OrchestratorError> {
    if !routing.is_scale_set() {
        return Ok(None);
    }
    let execution = prep
        .config
        .execution
        .as_ref()
        .ok_or_else(|| OrchestratorError::Contract {
            problem: "cache_housekeeping_scale_set_execution_missing".to_owned(),
        })?;
    let selector = execution
        .scale_selector()
        .map_err(|error| OrchestratorError::Contract {
            problem: error.to_string(),
        })?;
    let image = execution
        .profiles
        .get(&execution.scale_set_profile)
        .and_then(|profile| profile.job_container_image.clone());
    Ok(Some((selector, image)))
}

fn files_for(
    policy: PullRequestCachePolicy,
    runner_label: &str,
    routing: &WorkflowRoutingPolicy,
    selected: Option<(&ScaleSetSelector, Option<&ValidatedImageReference>)>,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    if policy != PullRequestCachePolicy::SameRepositoryScoped {
        return Ok(Vec::new());
    }
    let (runs_on, image) = route(runner_label, routing, selected)?;
    Ok(vec![
        velnor_actions_workflow_renderer::cache_cleanup::render_cache_cleanup_yml(
            &runs_on,
            image,
            env!("CARGO_PKG_VERSION"),
        )?,
    ])
}

fn route<'a>(
    runner_label: &str,
    routing: &WorkflowRoutingPolicy,
    selected: Option<(&'a ScaleSetSelector, Option<&'a ValidatedImageReference>)>,
) -> Result<(String, Option<&'a ValidatedImageReference>), OrchestratorError> {
    match (routing.is_scale_set(), selected) {
        (false, None) => Ok((runner_label.to_owned(), None)),
        (true, Some((selector, image))) => Ok((selector.token(), image)),
        (true, None) => Err(OrchestratorError::Contract {
            problem: "cache_housekeeping_scale_set_route_missing".to_owned(),
        }),
        (false, Some(_)) => Err(OrchestratorError::Contract {
            problem: "cache_housekeeping_hosted_route_has_scale_set_target".to_owned(),
        }),
    }
}

#[cfg(test)]
#[path = "cache_cleanup_emit_tests.rs"]
mod tests;
