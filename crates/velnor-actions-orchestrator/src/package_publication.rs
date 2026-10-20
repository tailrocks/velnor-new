//! Non-overwriting release transactions with a fresh gate before every mutation.
//!
//! Backends own authentication and transport. They must authenticate original
//! retained evidence before exposing an identity or remote observation. This
//! executor never treats a computed retry plan as continuing authorization.

#[path = "package_publication_rolling.rs"]
pub(crate) mod rolling;

use crate::{OrchestratorError, internal::internal};
use std::collections::BTreeSet;
use velnor_actions_contract::publication_retry::{
    PublicationAction, PublicationIdentity, PublicationState, publication_action,
};

/// Authenticated transport boundary for a single immutable release transaction.
///
/// Implementations must scope every API request to the receipt repository and
/// deterministic tag. No method may force a tag, replace an asset, delete a
/// release, or change authenticated original bytes. Transport failures propagate;
/// a later invocation starts again from independent remote observation.
pub trait PackagePublicationBackend {
    /// Authenticate the original receipt, signatures, attestation, workflow run,
    /// source authority, and available retained bytes. Return their exact identity.
    /// # Errors
    /// Rejects unauthenticated, changed, or unavailable original evidence.
    fn authenticate_original(&mut self) -> Result<PublicationIdentity, OrchestratorError>;
    /// Independently hash and authenticate the complete remote release inventory.
    /// An existing draft's origin must be bound to the original signed run.
    /// # Errors
    /// Rejects unavailable or unauthenticated remote observations.
    fn observe(&mut self) -> Result<PublicationState, OrchestratorError>;
    /// Return only original assets whose exact authenticated bytes remain available.
    /// # Errors
    /// Rejects unavailable or unverifiable retained byte inventories.
    fn available_original_assets(&mut self) -> Result<BTreeSet<String>, OrchestratorError>;
    /// Check exact-commit successful CI, current default-branch tip, workflow
    /// authority, protected-branch settings, and immutable-release prerequisites.
    /// A false result means stale candidate; malformed or unavailable proof errors.
    /// # Errors
    /// Rejects malformed, unavailable, or contradictory eligibility evidence.
    fn candidate_current(&mut self) -> Result<bool, OrchestratorError>;
    /// Consume observed action authority and recheck freshness before tag mutation.
    /// # Errors
    /// Rejects an existing reference, stale authority, or a failed tag mutation.
    fn create_tag(&mut self) -> Result<(), OrchestratorError>;
    /// Consume observed action authority and recheck freshness before creating a draft.
    /// # Errors
    /// Rejects an existing conflicting release or a failed draft mutation.
    fn create_draft(&mut self) -> Result<(), OrchestratorError>;
    /// Consume the exact observed release/action, then recheck live authority and upload.
    /// Existing names must conflict; no later release reselection is permitted.
    /// # Errors
    /// Rejects an existing asset name, changed original bytes, or a failed upload.
    fn upload_original_asset(&mut self, name: &str) -> Result<(), OrchestratorError>;
    /// Consume the exact observed draft/action and recheck authority before publication.
    /// # Errors
    /// Rejects incomplete assets, stale eligibility, or a failed publication.
    fn publish_draft(&mut self) -> Result<(), OrchestratorError>;
}

/// Successful terminal state after independently re-reading the visible release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackagePublicationOutcome {
    /// This invocation completed publication and verified its immutable result.
    Published,
    /// Original immutable publication already matched; downstream may reconcile.
    AlreadyPublished,
}

/// Complete or reconcile one authenticated immutable release transaction.
///
/// Re-observation after every mutation detects concurrent writers and partial
/// transport success. Every unfinished mutation independently rechecks freshness.
/// No automatic retry occurs after a transport error. At most one tag, one draft,
/// one upload per expected asset, and one publication are attempted.
///
/// # Errors
/// Rejects stale candidates, conflicting state, lost original evidence, mutable
/// publication, unexpected postconditions, and every backend authentication error.
pub fn publish_package_release(
    backend: &mut impl PackagePublicationBackend,
) -> Result<PackagePublicationOutcome, OrchestratorError> {
    let expected = backend.authenticate_original()?;
    if expected.assets.is_empty() {
        return Err(internal("package_publication_empty_inventory"));
    }
    let mut published_here = false;
    let max_steps = expected.assets.len().saturating_add(4);
    for _ in 0..max_steps {
        // Authentication repeats: local bytes and retained evidence are not a lease.
        if backend.authenticate_original()? != expected {
            return Err(internal("package_publication_original_changed"));
        }
        let observed = backend.observe()?;
        let available = backend.available_original_assets()?;
        // Eligibility here is only a proposal; each backend write consumes its
        // observation and checks live authority at its actual transport boundary.
        let current = true;
        match publication_action(&expected, &observed, current, &available) {
            PublicationAction::CreateTagAndDraft => {
                backend.create_tag()?;
            }
            PublicationAction::CreateDraft => {
                backend.create_draft()?;
            }
            PublicationAction::CompleteDraft { missing_assets } => {
                if let Some(name) = missing_assets.first() {
                    backend.upload_original_asset(name)?;
                } else {
                    backend.publish_draft()?;
                    published_here = true;
                }
            }
            PublicationAction::ReconcileDistribution => {
                return Ok(if published_here {
                    PackagePublicationOutcome::Published
                } else {
                    PackagePublicationOutcome::AlreadyPublished
                });
            }
            PublicationAction::Reject(reason) => {
                return Err(internal(&format!("package_publication_refused:{reason:?}")));
            }
        }
    }
    Err(internal("package_publication_postcondition_not_reached"))
}

#[cfg(test)]
#[path = "package_publication_tests.rs"]
mod tests;
