//! Read-only trust metadata from an Actions workflow-run REST response.

use serde_json::{Map, Value};

use crate::policy::{ActionsWorkflowTrustRun, ReusableWorkflowEvidence, WorkflowTrustField};
use crate::{SessionError, Transport, WireError};

use super::{actions_request, status_error};

const MAX_TRUST_TEXT: usize = 4096;

/// Fetch one workflow run with trust-relevant metadata preserved.
///
/// Unlike [`super::super::get_actions_workflow_run`], this DTO distinguishes
/// omitted/null fields from malformed fields for fail-closed policy evaluation.
/// It performs one repository-scoped GET and never polls a Scale Set session.
///
/// # Errors
///
/// Returns an input, transport, authorization, status, or malformed-response
/// error without exposing response bodies.
pub(crate) fn get_actions_workflow_trust_run<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    workflow_run_id: i64,
    actions_token: &str,
) -> Result<ActionsWorkflowTrustRun, SessionError>
where
    T: Transport + ?Sized,
{
    super::validate_repository(owner, repository, actions_token)?;
    if workflow_run_id <= 0 {
        return Err(WireError::RegistrationRejected.into());
    }
    let request = actions_request(
        format!("repos/{owner}/{repository}/actions/runs/{workflow_run_id}"),
        actions_token,
    )?;
    let exchange = crate::session::execute(transport, &request)?;
    if exchange.status != 200 {
        return Err(status_error(exchange.status));
    }
    let value: Value = serde_json::from_slice(&exchange.body).map_err(|_| WireError::Malformed)?;
    let object = value.as_object().ok_or(WireError::Malformed)?;
    let id = required_positive_i64(object, "id")?;
    let observed_run_attempt = required_positive_i64(object, "run_attempt")?;
    let event = required_string(object, "event")?;
    let path = required_string(object, "path")?;
    let head_sha = required_string(object, "head_sha")?;
    if id != workflow_run_id {
        return Err(WireError::Malformed.into());
    }
    let head_branch = optional_string(object, "head_branch");
    let head_repository_full_name = optional_object_string(object, "head_repository", "full_name");
    let referenced_workflows = optional_referenced_workflows(object);
    Ok(ActionsWorkflowTrustRun {
        id,
        observed_run_attempt,
        event,
        path,
        head_sha,
        head_branch,
        head_repository_full_name,
        referenced_workflows,
    })
}

fn required_positive_i64(object: &Map<String, Value>, key: &str) -> Result<i64, WireError> {
    object
        .get(key)
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
        .ok_or(WireError::Malformed)
}

fn required_string(object: &Map<String, Value>, key: &str) -> Result<String, WireError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| valid_text(value))
        .map(ToOwned::to_owned)
        .ok_or(WireError::Malformed)
}

fn optional_string(object: &Map<String, Value>, key: &str) -> WorkflowTrustField<String> {
    match object.get(key) {
        None | Some(Value::Null) => WorkflowTrustField::Missing,
        Some(Value::String(value)) if valid_text(value) => {
            WorkflowTrustField::Present(value.clone())
        }
        Some(_) => WorkflowTrustField::Invalid,
    }
}

fn optional_object_string(
    object: &Map<String, Value>,
    object_key: &str,
    value_key: &str,
) -> WorkflowTrustField<String> {
    match object.get(object_key) {
        None | Some(Value::Null) => WorkflowTrustField::Missing,
        Some(Value::Object(nested)) => optional_string(nested, value_key),
        Some(_) => WorkflowTrustField::Invalid,
    }
}

fn optional_referenced_workflows(
    object: &Map<String, Value>,
) -> WorkflowTrustField<Vec<ReusableWorkflowEvidence>> {
    let Some(value) = object.get("referenced_workflows") else {
        return WorkflowTrustField::Missing;
    };
    let Value::Array(items) = value else {
        return if value.is_null() {
            WorkflowTrustField::Missing
        } else {
            WorkflowTrustField::Invalid
        };
    };
    let mut workflows = Vec::with_capacity(items.len());
    for item in items {
        let Some(workflow) = item.as_object() else {
            return WorkflowTrustField::Invalid;
        };
        let Ok(path) = required_string(workflow, "path") else {
            return WorkflowTrustField::Invalid;
        };
        let Ok(sha) = required_string(workflow, "sha") else {
            return WorkflowTrustField::Invalid;
        };
        let git_ref = optional_string(workflow, "ref");
        workflows.push(ReusableWorkflowEvidence { path, git_ref, sha });
    }
    WorkflowTrustField::Present(workflows)
}

fn valid_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_TRUST_TEXT && !value.chars().any(char::is_control)
}
