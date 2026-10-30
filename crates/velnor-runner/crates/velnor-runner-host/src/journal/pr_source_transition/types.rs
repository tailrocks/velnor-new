use serde::{Deserialize, Serialize};

use super::receipt::{
    NonceCommitObservation, NonceCommitReceipt, ReferenceReceipt, RetargetHistoryReceipt,
    RetargetResponseReceipt,
};

/// Remote effect that reached an uncertain result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TransitionEffect {
    Retarget,
    NonceCommit,
    PublishReference,
}

/// Why a claimed remote effect cannot safely be repeated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UncertaintyReason {
    Transport,
    MalformedResponse,
    MismatchedResponse,
    PostEffectRepositoryCheck,
    PostEffectHistoryCheck,
    RestartedInFlight,
}

/// Durable state transitions, one appended event per committed boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
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
#[serde(tag = "response", rename_all = "snake_case")]
pub(crate) enum ObservedResponse {
    Retarget(RetargetResponseReceipt),
    RetargetHistory(RetargetHistoryReceipt),
    NonceCommit(NonceCommitObservation),
    ReferenceUpdate(ReferenceReceipt),
}

/// Progress recoverable from the committed event sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransitionStage {
    Prepared,
    RetargetInFlight,
    RetargetResponseObserved,
    Retargeted,
    NonceReserved,
    NonceCommitInFlight,
    NonceCommitCreated,
    ReferenceUpdateInFlight,
    Complete,
    Uncertain,
}

/// Effect response disposition after validating the exact expected identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResponseDisposition {
    Recorded,
    Uncertain,
}

/// Cause of a rejected plan or unsafe/incorrect transition record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum TransitionError {
    #[error("incorrect pull request transition plan")]
    IncorrectPlan,
    #[error("incorrect or repeated pull request transition effect")]
    IncorrectHistory,
    #[error("nonce generation failed")]
    RandomSource,
}
