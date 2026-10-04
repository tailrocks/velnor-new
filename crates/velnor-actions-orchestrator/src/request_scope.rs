//! Verification scope carried by the event request boundary.

use serde_json::Value;
use velnor_actions_contract::{VerificationScope, WorkflowEvent};

use crate::OrchestratorError;
use crate::internal::internal;

/// Resolve the requested verification scope from one triggering event.
///
/// Scheduled runs always verify the complete universe. Dispatches may opt
/// into that mode through the exact `inputs.scope` values `affected` and
/// `full`; an omitted input keeps the workflow's affected default. Other
/// events have no scope input and always use affected verification.
pub(crate) fn scope_for(
    event: WorkflowEvent,
    payload: &Value,
) -> Result<VerificationScope, OrchestratorError> {
    match event {
        WorkflowEvent::Schedule => Ok(VerificationScope::Full),
        WorkflowEvent::WorkflowDispatch => dispatch_scope(payload),
        _ => Ok(VerificationScope::Affected),
    }
}

/// Parse the optional dispatch scope input with exact spelling.
fn dispatch_scope(payload: &Value) -> Result<VerificationScope, OrchestratorError> {
    let Some(inputs) = payload.get("inputs") else {
        return Ok(VerificationScope::Affected);
    };
    let Some(inputs) = inputs.as_object() else {
        return Err(internal("malformed_scope"));
    };
    let Some(scope) = inputs.get("scope") else {
        return Ok(VerificationScope::Affected);
    };
    match scope.as_str() {
        Some("affected") => Ok(VerificationScope::Affected),
        Some("full") => Ok(VerificationScope::Full),
        _ => Err(internal("malformed_scope")),
    }
}

#[cfg(test)]
#[path = "request_scope_tests.rs"]
mod tests;
