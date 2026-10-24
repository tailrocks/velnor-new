//! Resolve a bound cache key against the run's staged BEFORE snapshot.

use velnor_actions_contract::workflow::QualificationCacheBackendObservation;

use crate::OrchestratorError;
use crate::internal::internal;

use super::QualificationCacheBindingRequest;

/// Resolve one bound restore key against the authenticated current-run snapshot.
///
/// # Errors
/// Returns an error if the staged plan, snapshot, key, or run binding is invalid.
pub fn qualification_cache_before_observation(
    request: &QualificationCacheBindingRequest<'_>,
    key: &str,
) -> Result<QualificationCacheBackendObservation, OrchestratorError> {
    let binding = super::bind_qualification_cache_runtime(request)?;
    if binding.restore_key.as_deref() != Some(key) {
        return Err(internal("qualification_cache_snapshot_key_not_bound"));
    }
    super::documents::observe_before_snapshot(request, key)
}
