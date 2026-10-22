use crate::error::HostError;
use crate::journal::Journal;
use velnor_runner_github::workflow_run::PullRequestNonceCommitReceipt;

use super::model::{NonceCommitReceipt, TransitionLedger};
use super::{TransitionEffect, TransitionPlan, TransitionStage};

/// Access the transition journal without exposing the journal database connection.
pub struct PrSourceTransitionJournal<'a> {
    pub(super) journal: &'a Journal,
}

impl std::fmt::Debug for PrSourceTransitionJournal<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PrSourceTransitionJournal")
            .finish_non_exhaustive()
    }
}

/// Strictly replayed immutable plan and append-only event view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionSnapshot {
    pub(super) ledger: TransitionLedger,
}

impl TransitionSnapshot {
    /// Immutable plan needed to continue read-only verification after restart.
    #[must_use]
    pub const fn plan(&self) -> &TransitionPlan {
        self.ledger.plan()
    }

    /// Stable identifier for one connection, repository, pull request and exact head commit.
    ///
    /// # Errors
    ///
    /// Returns an error if the frozen transition plan is invalid.
    pub fn transition_id(&self) -> Result<String, HostError> {
        self.ledger
            .plan()
            .transition_id()
            .map_err(|_| HostError::Journal)
    }

    /// Immutable digest of the exact full transition plan.
    ///
    /// # Errors
    ///
    /// Returns an error if the frozen transition plan is invalid.
    pub fn plan_digest(&self) -> Result<String, HostError> {
        self.ledger.plan().digest().map_err(|_| HostError::Journal)
    }

    /// Current state derived from the persisted events.
    #[must_use]
    pub fn stage(&self) -> TransitionStage {
        self.ledger.stage()
    }

    /// Number of committed events, used as the append compare-and-swap version.
    #[must_use]
    pub fn event_count(&self) -> usize {
        self.ledger.events().len()
    }

    /// Rebuild the validated helper receipt only while reference publication remains available.
    #[must_use]
    pub fn nonce_commit_receipt(&self) -> Option<PullRequestNonceCommitReceipt> {
        if self.ledger.stage() != TransitionStage::NonceCommitCreated {
            return None;
        }
        let nonce = self.ledger.nonce()?;
        let receipt: &NonceCommitReceipt = self.ledger.nonce_commit_receipt()?;
        Some(receipt.helper_receipt(self.ledger.plan(), nonce))
    }
}

/// Non-cloneable capability returned only after its effect claim commits.
#[must_use = "effect claims must be used only for their one corresponding GitHub helper call"]
#[derive(Debug)]
pub struct EffectClaim {
    pub(super) transition_id: String,
    pub(super) plan_digest: String,
    pub(super) event_count: usize,
    pub(super) effect: TransitionEffect,
    pub(super) nonce: Option<String>,
}

impl EffectClaim {
    /// Transition this capability belongs to.
    #[must_use]
    pub fn transition_id(&self) -> &str {
        &self.transition_id
    }

    /// Immutable plan digest covered by the persisted claim.
    #[must_use]
    pub fn plan_digest(&self) -> &str {
        &self.plan_digest
    }

    /// Committed event count immediately after the claim.
    #[must_use]
    pub const fn event_count(&self) -> usize {
        self.event_count
    }

    /// The one remote effect this capability records.
    #[must_use]
    pub const fn effect(&self) -> TransitionEffect {
        self.effect
    }

    /// Reserved nonce for a nonce-commit capability.
    #[must_use]
    pub fn nonce(&self) -> Option<&str> {
        self.nonce.as_deref()
    }
}

/// Result of explicit crash recovery. This value never authorizes a remote effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionRecovery {
    /// No effect was in flight when recovery ran.
    NoInterruptedEffect,
    /// Recovery durably recorded sticky uncertainty for an outstanding effect claim.
    RecordedUncertainty,
    /// A retarget response exists; only a read-only complete-history check may continue.
    VerifyRetargetHistoryReadOnly,
}

pub(super) fn effect_claim(
    transition_id: &str,
    ledger: &TransitionLedger,
    effect: TransitionEffect,
    nonce: Option<String>,
) -> Result<EffectClaim, HostError> {
    Ok(EffectClaim {
        transition_id: transition_id.to_owned(),
        plan_digest: ledger.plan().digest().map_err(|_| HostError::Journal)?,
        event_count: ledger.events().len(),
        effect,
        nonce,
    })
}
