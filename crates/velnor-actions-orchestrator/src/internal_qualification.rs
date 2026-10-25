//! Qualification-only plan request and runner-image checks.

use velnor_actions_contract::{Plan, RunsOn, WorkflowEvent};

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::internal_request::PlanRequest;
use crate::prepare::GenerationPreparation;

/// Validate qualification context and copy the configured image source into it.
pub(super) fn bind_image_source(
    request: &mut PlanRequest,
    prep: &GenerationPreparation,
) -> Result<Option<String>, OrchestratorError> {
    validate_request(
        request,
        &prep.default_branch,
        prep.config.workflow.verification_branch.as_deref(),
    )?;
    let source = prep
        .config
        .execution
        .as_ref()
        .and_then(|execution| execution.scale_set_image_source_sha())
        .map(str::to_owned);
    if let Some(context) = request.qualification.as_mut() {
        context.scale_set_image_source_sha.clone_from(&source);
        context.validate_shape().map_err(internal_contract)?;
    }
    Ok(source)
}

/// Require an explicit immutable image source for every Scale Set qualification lane.
pub(super) fn validate_scale_set_image_source(
    request: &PlanRequest,
    prep: &GenerationPreparation,
    plan: &Plan,
    source: Option<&str>,
) -> Result<(), OrchestratorError> {
    let has_scale_set_lane = plan.matrix.include.iter().any(|entry| {
        RunsOn::parse(&entry.planned_platform.runs_on).is_ok_and(|runs_on| runs_on.is_scale_set())
    });
    if request.event != WorkflowEvent::Qualification || !has_scale_set_lane || source.is_some() {
        return Ok(());
    }
    let profile = prep
        .config
        .execution
        .as_ref()
        .map_or("scale_set", |execution| {
            execution.scale_set_profile.as_str()
        });
    Err(OrchestratorError::config(
        crate::config::CONFIG_REL,
        format!("execution.profiles.{profile}.image_source_sha"),
        "required_for_scale_set_qualification",
    ))
}

/// Bind qualification provenance to the prepared repository and checkout.
fn validate_request(
    request: &PlanRequest,
    default_branch: &str,
    verification_branch: Option<&str>,
) -> Result<(), OrchestratorError> {
    match (request.event, request.qualification.as_ref()) {
        (WorkflowEvent::Qualification, Some(context)) => {
            let repository = request
                .repository
                .as_deref()
                .ok_or_else(|| internal("missing_qualification_repository"))?;
            context
                .validate_for_config(
                    default_branch,
                    verification_branch,
                    repository,
                    &request.head,
                )
                .map_err(internal_contract)
        }
        (WorkflowEvent::Qualification, None) => Err(internal("missing_qualification_context")),
        (_, Some(_)) => Err(internal("qualification_context_on_wrong_event")),
        (_, None) => Ok(()),
    }
}
