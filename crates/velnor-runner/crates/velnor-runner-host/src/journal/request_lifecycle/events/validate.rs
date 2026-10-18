//! Validation for native Started and Completed event metadata.

use super::storage;
use super::{EventSource, MAX_EVENT_TEXT};
use crate::error::HostError;
use velnor_runner_github::{RunnerReference, SourceMetadata};

pub(super) fn validate_event(
    message_id: i64,
    runner: &RunnerReference,
    job_id: Option<&str>,
) -> Result<(), HostError> {
    if message_id < 0 {
        return Err(HostError::Identity);
    }
    validate_runner(runner)?;
    validate_optional_event_text(job_id)
}

pub(super) fn validate_runner(runner: &RunnerReference) -> Result<(), HostError> {
    if runner.id <= 0
        || runner.runner_scale_set_id <= 0
        || runner.name.is_empty()
        || runner.name.len() > 128
        || runner.name.chars().any(char::is_control)
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

pub(super) fn validate_optional_event_text(value: Option<&str>) -> Result<(), HostError> {
    if value.is_some_and(|text| text.len() > MAX_EVENT_TEXT || text.chars().any(char::is_control)) {
        return Err(HostError::Identity);
    }
    Ok(())
}

pub(super) fn validate_source(source: &SourceMetadata) -> Result<EventSource<'_>, HostError> {
    for (value, maximum) in [
        (source.owner_name.as_deref(), 256),
        (source.repository_name.as_deref(), 256),
        (source.job_workflow_ref.as_deref(), 4096),
        (source.event_name.as_deref(), 128),
    ] {
        if value.is_some_and(|text| {
            text.trim().is_empty() || text.len() > maximum || text.chars().any(char::is_control)
        }) {
            return Err(HostError::Identity);
        }
    }
    if source.workflow_run_id.is_some_and(|value| value <= 0) {
        return Err(HostError::Identity);
    }
    Ok(EventSource {
        owner_name: source.owner_name.as_deref(),
        repository_name: source.repository_name.as_deref(),
        job_workflow_ref: source.job_workflow_ref.as_deref(),
        workflow_run_id: source.workflow_run_id,
        event_name: source.event_name.as_deref(),
    })
}

pub(super) fn validate_job_id_history(
    state: &storage::RequestEventState,
    incoming: Option<&str>,
) -> Result<(), HostError> {
    let prior = match (
        state.started_job_id.as_deref(),
        state.completed_job_id.as_deref(),
    ) {
        (Some(started), Some(completed)) if started != completed => {
            return Err(HostError::Journal);
        }
        (Some(started), _) => Some(started),
        (_, Some(completed)) => Some(completed),
        (None, None) => None,
    };
    if prior.is_some_and(|value| incoming != Some(value)) {
        return Err(HostError::Identity);
    }
    Ok(())
}
