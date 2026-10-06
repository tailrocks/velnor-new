//! Merge-time verification-scope coherence.

use std::collections::BTreeSet;

use serde_json::Value;
use velnor_actions_contract::{Plan, VerificationScope, WorkflowEvent};

use super::MergeRequest;
use crate::cover::Signals;
use crate::request_event::request_scope;

/// Require the plan scope to equal the scope observed from the runner.
///
/// Scope is mandatory whenever merge has a plan. Dispatch scope is selected
/// by its event payload and therefore only needs to be present here; the
/// other event kinds have one canonical scope that can be re-derived without
/// the payload. All failures use the existing closed trust/scope diagnostic
/// token.
pub(crate) fn check_scope_coherence(
    plan: &Plan,
    request: &MergeRequest,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    let coherent = match (request.actual_event, request.actual_scope) {
        (None, None) => false,
        (Some(event), Some(actual_scope)) => {
            plan.scope == actual_scope && runner_scope_matches_event(event, actual_scope)
        }
        _ => false,
    };
    if !coherent {
        signals.planning_failed = true;
        miss_reasons.insert("trust_scope_mismatch".to_owned());
    }
    if !plan.producers.entries.is_empty()
        && (request.actual_producer_context.is_none()
            || request.actual_producer_context != plan.producers.context)
    {
        signals.planning_failed = true;
        miss_reasons.insert("trust_scope_mismatch".to_owned());
    }
}

/// Check the event's canonical scope where the event has no user-selected
/// scope input. Manual dispatch is intentionally open because its payload
/// input has already been parsed and persisted as `actual_scope` at assembly.
fn runner_scope_matches_event(event: WorkflowEvent, actual_scope: VerificationScope) -> bool {
    if event == WorkflowEvent::WorkflowDispatch {
        return true;
    }
    request_scope::scope_for(event, &Value::Null)
        .map(|scope| scope == actual_scope)
        .unwrap_or(false)
}

#[cfg(test)]
#[path = "merge_scope_tests.rs"]
mod merge_scope_tests;
