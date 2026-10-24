//! Detached merge support for synthetic integration fixtures.

use velnor_actions_orchestrator::{MergeContext, OrchestratorError, merge_internal_with_context};

/// Merge a synthetic fixture without ambient repository identity.
pub(crate) fn merge_fixture(request_json: &str) -> Result<String, OrchestratorError> {
    merge_internal_with_context(request_json, &MergeContext::detached())
}
