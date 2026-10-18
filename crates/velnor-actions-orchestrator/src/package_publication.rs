//! Non-overwriting release transactions with a fresh gate before every mutation.
//!
//! Backends own authentication and transport. They must authenticate original
//! retained evidence before exposing an identity or remote observation. This
//! executor never treats a computed retry plan as continuing authorization.

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
    /// Create the deterministic receipt-bound annotated tag without force or replacement.
    /// # Errors
    /// Rejects an existing reference, stale authority, or a failed tag mutation.
    fn create_tag(&mut self) -> Result<(), OrchestratorError>;
    /// Create an unpublished prerelease draft without replacing an existing release.
    /// # Errors
    /// Rejects an existing conflicting release or a failed draft mutation.
    fn create_draft(&mut self) -> Result<(), OrchestratorError>;
    /// Upload one missing original asset exclusively. Existing names must conflict.
    /// # Errors
    /// Rejects an existing asset name, changed original bytes, or a failed upload.
    fn upload_original_asset(&mut self, name: &str) -> Result<(), OrchestratorError>;
    /// Publish the complete draft as an immutable prerelease, never as latest.
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
        let current = if matches!(observed, PublicationState::Published { .. }) {
            false
        } else {
            backend.candidate_current()?
        };
        match publication_action(&expected, &observed, current, &available) {
            PublicationAction::CreateTagAndDraft => {
                require_current(backend)?;
                backend.create_tag()?;
            }
            PublicationAction::CreateDraft => {
                require_current(backend)?;
                backend.create_draft()?;
            }
            PublicationAction::CompleteDraft { missing_assets } => {
                require_current(backend)?;
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

fn require_current(backend: &mut impl PackagePublicationBackend) -> Result<(), OrchestratorError> {
    if backend.candidate_current()? {
        Ok(())
    } else {
        Err(internal("package_publication_refused:StaleCandidate"))
    }
}

#[cfg(test)]
#[path = "package_publication_tests.rs"]
mod tests;
