use serde::{Deserialize, Serialize};

use super::receipt::{
    NonceCommitObservation, NonceCommitReceipt, ReferenceReceipt, RetargetHistoryReceipt,
    RetargetResponseReceipt,
};

/// Remote effect that reached an uncertain result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TransitionEffect {
    /// Pull request base update.
    Retarget,
    /// Nonce commit object creation.
    NonceCommit,
    /// Non-force update to the pull request head reference.
    PublishReference,
}

/// Why a claimed remote effect cannot safely be repeated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum UncertaintyReason {
    /// Transport failed after the remote effect claim.
    Transport,
    /// Response could not be decoded or failed bounded shape checks.
    MalformedResponse,
    /// Response identity did not match the frozen plan.
    MismatchedResponse,
    /// Read-only repository identity check failed after the effect.
    PostEffectRepositoryCheck,
    /// Read-only complete timeline check failed after retarget.
    PostEffectHistoryCheck,
    /// Process restarted while a remote effect was in flight.
    RestartedInFlight,
}

/// Durable state transitions, one appended event per committed boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum TransitionEvent {
    RetargetClaimed,
    RetargetResponseObserved {
        receipt: RetargetResponseReceipt,
    },
    RetargetAcknowledged {
        response: RetargetResponseReceipt,
        history: RetargetHistoryReceipt,
    },
    NonceReserved {
        nonce: String,
    },
    NonceCommitClaimed,
    NonceCommitRecorded {
        receipt: NonceCommitReceipt,
    },
    ReferenceUpdateClaimed,
    ReferencePublished {
        receipt: ReferenceReceipt,
    },
    EffectUncertain {
        effect: TransitionEffect,
        reason: UncertaintyReason,
        observed_response: Option<ObservedResponse>,
    },
}

/// Bounded, non-secret response facts retained when a claimed effect is uncertain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ObservedResponse {
    Retarget(RetargetResponseReceipt),
    RetargetHistory(RetargetHistoryReceipt),
    NonceCommit(NonceCommitObservation),
    ReferenceUpdate(ReferenceReceipt),
}

/// Progress recoverable from the committed event sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionStage {
    /// No effects started.
    Prepared,
    /// Base update claim committed; PATCH may have started.
    RetargetInFlight,
    /// Base update response committed; only timeline read is allowed.
    RetargetResponseObserved,
    /// Base update verified by complete timeline.
    Retargeted,
    /// Random nonce committed to the journal.
    NonceReserved,
    /// Commit creation claim committed; POST may have started.
    NonceCommitInFlight,
    /// Exact nonce commit receipt committed.
    NonceCommitCreated,
    /// Reference update claim committed; PATCH may have started.
    ReferenceUpdateInFlight,
    /// Reference update receipt committed.
    Complete,
    /// A claimed effect reached sticky uncertainty.
    Uncertain,
}

/// Effect response disposition after validating the exact expected identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseDisposition {
    /// Exact expected response committed.
    Recorded,
    /// Mismatch committed as sticky uncertainty.
    Uncertain,
}

/// Cause of a rejected plan or unsafe/incorrect transition record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TransitionError {
    /// Frozen plan fields failed validation.
    #[error("incorrect pull request transition plan")]
    IncorrectPlan,
    /// Event sequence or remote response failed validation.
    #[error("incorrect or repeated pull request transition effect")]
    IncorrectHistory,
    /// Secure random nonce generation failed.
    #[error("nonce generation failed")]
    RandomSource,
}
