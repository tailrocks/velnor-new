use super::{TransitionLedger, TransitionStage};
use crate::journal::pr_source_transition::model::receipt::{
    NonceCommitReceipt, ReferenceReceipt, nonce_message_digest,
};
use crate::journal::pr_source_transition::model::types::{
    ObservedResponse, TransitionEffect, TransitionEvent, UncertaintyReason,
};
use crate::journal::pr_source_transition::model::validation::{
    NONCE_BYTES, valid_hex, valid_opaque_id, valid_ref, valid_sha,
};

impl TransitionLedger {
    pub(super) fn latest_commit_sha(&self) -> Option<&str> {
        self.events.iter().find_map(|event| match event {
            TransitionEvent::NonceCommitRecorded { receipt } => Some(receipt.sha.as_str()),
            _ => None,
        })
    }

    pub(super) fn event_is_valid(&self, event: &TransitionEvent) -> bool {
        match (event, self.stage()) {
            (TransitionEvent::RetargetClaimed, TransitionStage::Prepared)
            | (TransitionEvent::NonceCommitClaimed, TransitionStage::NonceReserved)
            | (TransitionEvent::ReferenceUpdateClaimed, TransitionStage::NonceCommitCreated) => {
                true
            }
            (
                TransitionEvent::RetargetResponseObserved { receipt },
                TransitionStage::RetargetInFlight,
            ) => receipt.matches(&self.plan),
            (
                TransitionEvent::RetargetAcknowledged { response, history },
                TransitionStage::RetargetResponseObserved,
            ) => self
                .retarget_response()
                .is_some_and(|prior| response == prior && history.matches(&self.plan, prior)),
            (TransitionEvent::NonceReserved { nonce }, TransitionStage::Retargeted) => {
                valid_hex(nonce, NONCE_BYTES * 2)
            }
            (
                TransitionEvent::NonceCommitRecorded { receipt },
                TransitionStage::NonceCommitInFlight,
            ) => self.commit_receipt_matches(receipt),
            (
                TransitionEvent::ReferencePublished { receipt },
                TransitionStage::ReferenceUpdateInFlight,
            ) => self.reference_matches(receipt),
            (
                TransitionEvent::EffectUncertain {
                    effect,
                    reason,
                    observed_response,
                },
                stage,
            ) => {
                effect_for_stage(stage).is_some_and(|expected| expected == *effect)
                    && observed_response_matches_effect(*effect, observed_response.as_ref())
                    && observed_response_is_valid(observed_response.as_ref())
                    && uncertainty_reason_matches_observation(*reason, observed_response.is_some())
            }
            _ => false,
        }
    }

    pub(super) fn commit_receipt_matches(&self, receipt: &NonceCommitReceipt) -> bool {
        self.nonce().is_some_and(|nonce| {
            receipt.parent_sha == self.plan.head_sha
                && valid_sha(&receipt.sha)
                && valid_sha(&receipt.tree_sha)
                && receipt.tree_sha == self.plan.head_tree_sha
                && valid_hex(&receipt.message_sha256, 64)
                && receipt.message_sha256 == nonce_message_digest(nonce, &self.plan)
                && receipt.nonce_marker_count == 1
                && receipt.sign_off_count == 1
                && receipt.author_name == self.plan.commit_identity.name()
                && receipt.author_email == self.plan.commit_identity.email()
                && receipt.committer_name == self.plan.commit_identity.name()
                && receipt.committer_email == self.plan.commit_identity.email()
                && valid_hex(nonce, NONCE_BYTES * 2)
        })
    }

    pub(super) fn reference_matches(&self, receipt: &ReferenceReceipt) -> bool {
        self.latest_commit_sha().is_some_and(|sha| {
            receipt.reference == format!("refs/heads/{}", self.plan.head_ref)
                && receipt.object_type == "commit"
                && receipt.sha == sha
        })
    }
}

fn effect_for_stage(stage: TransitionStage) -> Option<TransitionEffect> {
    match stage {
        TransitionStage::RetargetInFlight | TransitionStage::RetargetResponseObserved => {
            Some(TransitionEffect::Retarget)
        }
        TransitionStage::NonceCommitInFlight => Some(TransitionEffect::NonceCommit),
        TransitionStage::ReferenceUpdateInFlight => Some(TransitionEffect::PublishReference),
        TransitionStage::Prepared
        | TransitionStage::Retargeted
        | TransitionStage::NonceReserved
        | TransitionStage::NonceCommitCreated
        | TransitionStage::Complete
        | TransitionStage::Uncertain => None,
    }
}

fn observed_response_matches_effect(
    effect: TransitionEffect,
    response: Option<&ObservedResponse>,
) -> bool {
    matches!(
        (effect, response),
        (
            TransitionEffect::Retarget,
            None | Some(ObservedResponse::Retarget(_) | ObservedResponse::RetargetHistory(_)),
        ) | (
            TransitionEffect::NonceCommit,
            None | Some(ObservedResponse::NonceCommit(_)),
        ) | (
            TransitionEffect::PublishReference,
            None | Some(ObservedResponse::ReferenceUpdate(_)),
        )
    )
}

fn observed_response_is_valid(response: Option<&ObservedResponse>) -> bool {
    match response {
        None => true,
        Some(ObservedResponse::Retarget(receipt)) => {
            receipt.pull_request_number > 0
                && receipt.base_repository_id > 0
                && receipt.head_repository_id > 0
                && valid_ref(&receipt.head_ref)
                && valid_ref(&receipt.previous_base_ref)
                && valid_ref(&receipt.current_base_ref)
                && valid_sha(&receipt.head_sha)
                && valid_sha(&receipt.previous_base_sha)
                && valid_sha(&receipt.current_base_sha)
        }
        Some(ObservedResponse::RetargetHistory(receipt)) => {
            receipt.pull_request_number > 0
                && receipt.base_repository_id > 0
                && receipt.head_repository_id > 0
                && valid_ref(&receipt.head_ref)
                && valid_ref(&receipt.base_ref)
                && !receipt.event_created_at.is_empty()
                && receipt.event_created_at.len() <= 64
                && valid_ref(&receipt.event_previous_ref)
                && valid_ref(&receipt.event_current_ref)
                && valid_sha(&receipt.head_sha)
                && valid_sha(&receipt.base_sha)
                && valid_opaque_id(&receipt.event_id)
                && valid_hex(&receipt.previous_history_digest, 64)
                && valid_hex(&receipt.complete_history_digest, 64)
                && receipt.complete_event_count <= 6400
        }
        Some(ObservedResponse::NonceCommit(observation)) => {
            valid_sha(&observation.sha)
                && valid_sha(&observation.tree_sha)
                && observation.parent_count <= 32
                && valid_hex(&observation.parents_sha256, 64)
                && valid_hex(&observation.message_sha256, 64)
                && observation.author.as_ref().is_none_or(|person| {
                    valid_hex(&person.name_sha256, 64) && valid_hex(&person.email_sha256, 64)
                })
                && observation.committer.as_ref().is_none_or(|person| {
                    valid_hex(&person.name_sha256, 64) && valid_hex(&person.email_sha256, 64)
                })
        }
        Some(ObservedResponse::ReferenceUpdate(receipt)) => {
            receipt.reference.len() <= 512
                && receipt.object_type.len() <= 64
                && valid_sha(&receipt.sha)
        }
    }
}

fn uncertainty_reason_matches_observation(
    reason: UncertaintyReason,
    has_observation: bool,
) -> bool {
    match reason {
        UncertaintyReason::MismatchedResponse => has_observation,
        UncertaintyReason::Transport
        | UncertaintyReason::MalformedResponse
        | UncertaintyReason::PostEffectRepositoryCheck
        | UncertaintyReason::PostEffectHistoryCheck
        | UncertaintyReason::RestartedInFlight => !has_observation,
    }
}
