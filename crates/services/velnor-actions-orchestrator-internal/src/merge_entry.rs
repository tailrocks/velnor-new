//! Merge entrypoint over the cover port implementation.
//!
//! Merge behavior lives in `velnor-actions-orchestrator-merge` and calls
//! cover only through the merge-ports cover trait, which the cover crate
//! implements; this module keeps the original `merge_internal`
//! entrypoint byte-identical for the CLI and integration tests.

use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_cover::cover_port::Cover;
use velnor_actions_orchestrator_merge::merge_internal_with;

/// Aggregate matrix reports into the final gate report (schema-1 JSON).
///
/// Only the request envelope (JSON shape, schema, run key) fails
/// outright; every evidence failure yields a diagnostic `planning_failed`
/// verdict with closed failure tokens. A missing plan still yields a
/// verdict instead of an error. Consumes the emitted plan only and never
/// rediscovers repository state.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for malformed requests and
/// response-encoding failures.
pub fn merge_internal(request_json: &str) -> Result<String, OrchestratorError> {
    merge_internal_with(&Cover, request_json)
}
