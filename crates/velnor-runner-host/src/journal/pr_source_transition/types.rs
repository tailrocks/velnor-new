// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use super::receipt::{
    NonceCommitObservation, NonceCommitReceipt, ReferenceReceipt, RetargetHistoryReceipt,
    RetargetResponseReceipt,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TransitionEffect {
    Retarget,

    NonceCommit,

    PublishReference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum UncertaintyReason {
    Transport,

    MalformedResponse,

    MismatchedResponse,

    PostEffectRepositoryCheck,

    PostEffectHistoryCheck,

    RestartedInFlight,
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ObservedResponse {
    Retarget(RetargetResponseReceipt),
    RetargetHistory(RetargetHistoryReceipt),
    NonceCommit(NonceCommitObservation),
    ReferenceUpdate(ReferenceReceipt),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionStage {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseDisposition {
    Recorded,

    Uncertain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TransitionError {
    #[error("incorrect pull request transition plan")]
    IncorrectPlan,

    #[error("incorrect or repeated pull request transition effect")]
    IncorrectHistory,

    #[error("nonce generation failed")]
    RandomSource,
}
