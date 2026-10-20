//! Rolling transitions in the native package publication executor.
//!
//! Transport ambiguity stops the invocation. Recovery always reauthenticates
//! original bytes and re-observes live IDs before choosing another mutation.

use super::PackagePublicationOutcome;
use crate::{OrchestratorError, internal::internal};
use std::collections::BTreeSet;
use velnor_actions_contract::publication_retry::{
    PublicationIdentity,
    rolling::{RollingAction, RollingState, rolling_action},
};

/// Credentialed adapter for the one configured mutable preview release.
pub(crate) trait RollingPackagePublicationBackend {
    /// Fixed configured existing release ID, never selected from remote hints.
    fn release_id(&self) -> u64;
    /// Reauthenticate signed original bytes, source, workflow and retained run.
    fn authenticate_original(&mut self) -> Result<PublicationIdentity, OrchestratorError>;
    /// Independently authenticate complete assets, predecessor and durable journal.
    fn observe(&mut self) -> Result<RollingState, OrchestratorError>;
    /// Names whose original signed bytes remain verified and available locally.
    fn available_original_assets(&mut self) -> Result<BTreeSet<String>, OrchestratorError>;
    /// Exact latest-main CI, workflow, protected environment and source authority.
    fn candidate_current(&mut self) -> Result<bool, OrchestratorError>;
    /// Execute exactly one mutation, using only fixed routes and captured IDs.
    /// Consume the last authenticated observation and recheck mutable candidate
    /// authority immediately before transport. Never reuse a consumed observation.
    /// Reject terminal actions. Never re-draft, re-publish or recreate the release.
    fn mutate(&mut self, action: RollingAction) -> Result<(), OrchestratorError>;
}

pub(crate) fn publish_rolling_package_release(
    backend: &mut impl RollingPackagePublicationBackend,
) -> Result<PackagePublicationOutcome, OrchestratorError> {
    let expected = backend.authenticate_original()?;
    let release_id = backend.release_id();
    let mut committed_here = false;
    // Each desired object can require one starter deletion, upload and rename.
    // Old assets may additionally require one deletion each. No transport error
    // consumes this allowance: it returns immediately instead of blindly retrying.
    let mut remaining = None;
    loop {
        if backend.release_id() != release_id || backend.authenticate_original()? != expected {
            return Err(internal("rolling_publication_original_changed"));
        }
        let observed = backend.observe()?;
        let available = backend.available_original_assets()?;
        // This is a proposal, never write authority. The adapter consumes its
        // observation and checks fresh candidate authority before each write.
        let action = rolling_action(&expected, release_id, &observed, true, &available);
        match action {
            RollingAction::Reconcile => {
                if !backend.candidate_current()? {
                    return Err(internal("rolling_publication_refused:StaleCandidate"));
                }
                return Ok(if committed_here {
                    PackagePublicationOutcome::Published
                } else {
                    PackagePublicationOutcome::AlreadyPublished
                });
            }
            RollingAction::Reject(reason) => {
                return Err(internal(&format!("rolling_publication_refused:{reason:?}")));
            }
            mutation => {
                let budget = remaining.get_or_insert_with(|| {
                    expected
                        .assets
                        .len()
                        .saturating_mul(3)
                        .saturating_add(observed.assets.len())
                        .saturating_add(5)
                });
                if *budget == 0 {
                    return Err(internal("rolling_publication_postcondition_not_reached"));
                }
                *budget -= 1;
                let committing = mutation == RollingAction::Commit;
                backend.mutate(mutation)?;
                committed_here |= committing;
            }
        }
    }
}

#[cfg(test)]
#[path = "package_publication_rolling_tests.rs"]
mod tests;
