use getrandom::fill;
use serde::{Deserialize, Serialize};
use velnor_runner_github::workflow_run::GitCommit;

use super::plan::TransitionPlan;
use super::receipt::{
    NonceCommitObservation, NonceCommitReceipt, ReferenceReceipt, RetargetHistoryReceipt,
    RetargetResponseReceipt,
};
use super::types::{
    ObservedResponse, ResponseDisposition, TransitionEffect, TransitionError, TransitionEvent,
    TransitionStage, UncertaintyReason,
};
use super::validation::{NONCE_BYTES, has_one_line, hex, valid_hex, valid_sha};

/// Pure append-only PR source transition record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TransitionLedger {
    plan: TransitionPlan,
    events: Vec<TransitionEvent>,
}

impl TransitionLedger {
    /// Start one transition from a validated immutable plan.
    pub(crate) fn new(plan: TransitionPlan) -> Result<Self, TransitionError> {
        plan.validate()?;
        Ok(Self {
            plan,
            events: Vec::new(),
        })
    }

    /// Rebuild a persisted row and reject an unsafe event history.
    pub(crate) fn from_events(
        plan: TransitionPlan,
        events: Vec<TransitionEvent>,
    ) -> Result<Self, TransitionError> {
        let mut ledger = Self {
            plan,
            events: Vec::new(),
        };
        ledger.plan.validate()?;
        for event in events {
            ledger.apply(event)?;
        }
        Ok(ledger)
    }

    /// Exact frozen transition inputs.
    #[must_use]
    pub(crate) const fn plan(&self) -> &TransitionPlan {
        &self.plan
    }

    /// Committed event order.
    #[must_use]
    pub(crate) fn events(&self) -> &[TransitionEvent] {
        &self.events
    }

    /// Current state derived from the last committed event.
    #[must_use]
    pub(crate) fn stage(&self) -> TransitionStage {
        self.events
            .last()
            .map_or(TransitionStage::Prepared, |event| match event {
                TransitionEvent::RetargetClaimed => TransitionStage::RetargetInFlight,
                TransitionEvent::RetargetResponseObserved { .. } => {
                    TransitionStage::RetargetResponseObserved
                }
                TransitionEvent::RetargetAcknowledged { .. } => TransitionStage::Retargeted,
                TransitionEvent::NonceReserved { .. } => TransitionStage::NonceReserved,
                TransitionEvent::NonceCommitClaimed => TransitionStage::NonceCommitInFlight,
                TransitionEvent::NonceCommitRecorded { .. } => TransitionStage::NonceCommitCreated,
                TransitionEvent::ReferenceUpdateClaimed => TransitionStage::ReferenceUpdateInFlight,
                TransitionEvent::ReferencePublished { .. } => TransitionStage::Complete,
                TransitionEvent::EffectUncertain { .. } => TransitionStage::Uncertain,
            })
    }

    /// Commit the one-shot retarget claim before sending the PATCH.
    pub(crate) fn claim_retarget(&mut self) -> Result<(), TransitionError> {
        self.append(TransitionEvent::RetargetClaimed)
    }

    /// Record the exact PATCH response before checking the resulting timeline.
    pub(crate) fn record_retarget_response(
        &mut self,
        receipt: RetargetResponseReceipt,
    ) -> Result<ResponseDisposition, TransitionError> {
        let matches = receipt.matches(&self.plan);
        let event = if matches {
            TransitionEvent::RetargetResponseObserved { receipt }
        } else {
            TransitionEvent::EffectUncertain {
                effect: TransitionEffect::Retarget,
                reason: UncertaintyReason::MismatchedResponse,
                observed_response: Some(ObservedResponse::Retarget(receipt)),
            }
        };
        self.append(event)?;
        Ok(disposition(matches))
    }

    /// Acknowledge the retarget only after a complete post-effect history check succeeds.
    pub(crate) fn record_retarget_verification(
        &mut self,
        receipt: RetargetHistoryReceipt,
    ) -> Result<ResponseDisposition, TransitionError> {
        let Some(TransitionEvent::RetargetResponseObserved { receipt: response }) =
            self.events.last()
        else {
            return Err(TransitionError::IncorrectHistory);
        };
        let matches = receipt.matches(&self.plan, response);
        let event = if matches {
            TransitionEvent::RetargetAcknowledged {
                response: response.clone(),
                history: receipt,
            }
        } else {
            TransitionEvent::EffectUncertain {
                effect: TransitionEffect::Retarget,
                reason: UncertaintyReason::PostEffectHistoryCheck,
                observed_response: Some(ObservedResponse::RetargetHistory(receipt)),
            }
        };
        self.append(event)?;
        Ok(disposition(matches))
    }

    /// Record a transport or decode failure after the PATCH was claimed.
    pub(crate) fn retarget_uncertain(
        &mut self,
        reason: UncertaintyReason,
    ) -> Result<(), TransitionError> {
        self.mark_uncertain(TransitionEffect::Retarget, reason)
    }

    /// Generate and commit the unpredictable nonce after an exact retarget acknowledgement.
    pub(crate) fn reserve_nonce(&mut self) -> Result<String, TransitionError> {
        if self.stage() != TransitionStage::Retargeted {
            return Err(TransitionError::IncorrectHistory);
        }
        let nonce = Nonce::generate()?;
        self.append(TransitionEvent::NonceReserved {
            nonce: nonce.0.clone(),
        })?;
        Ok(nonce.0)
    }

    /// Nonce already committed for this transition.
    #[must_use]
    pub(crate) fn nonce(&self) -> Option<&str> {
        self.events.iter().find_map(|event| match event {
            TransitionEvent::NonceReserved { nonce } => Some(nonce.as_str()),
            _ => None,
        })
    }

    /// Commit the one-shot commit creation claim before sending the POST.
    pub(crate) fn claim_nonce_commit(&mut self) -> Result<(), TransitionError> {
        self.append(TransitionEvent::NonceCommitClaimed)
    }

    /// Record the exact single-parent, configured-identity nonce commit response.
    pub(crate) fn record_nonce_commit(
        &mut self,
        commit: &GitCommit,
    ) -> Result<ResponseDisposition, TransitionError> {
        let nonce = self.nonce().ok_or(TransitionError::IncorrectHistory)?;
        let receipt = NonceCommitReceipt::from_commit(commit, &self.plan, nonce);
        let event = match receipt {
            Some(receipt) => TransitionEvent::NonceCommitRecorded { receipt },
            None => TransitionEvent::EffectUncertain {
                effect: TransitionEffect::NonceCommit,
                reason: UncertaintyReason::MismatchedResponse,
                observed_response: Some(ObservedResponse::NonceCommit(
                    NonceCommitObservation::from_commit(commit)?,
                )),
            },
        };
        let recorded = matches!(event, TransitionEvent::NonceCommitRecorded { .. });
        self.append(event)?;
        Ok(disposition(recorded))
    }

    /// Record a transport or decode failure after commit creation was claimed.
    pub(crate) fn nonce_commit_uncertain(
        &mut self,
        reason: UncertaintyReason,
    ) -> Result<(), TransitionError> {
        self.mark_uncertain(TransitionEffect::NonceCommit, reason)
    }

    /// Commit the one-shot non-force reference update claim before sending PATCH.
    pub(crate) fn claim_reference_update(&mut self) -> Result<(), TransitionError> {
        self.append(TransitionEvent::ReferenceUpdateClaimed)
    }

    /// Record the exact published commit at the configured head reference.
    pub(crate) fn record_reference_update(
        &mut self,
        receipt: ReferenceReceipt,
    ) -> Result<ResponseDisposition, TransitionError> {
        let commit_sha = self
            .latest_commit_sha()
            .ok_or(TransitionError::IncorrectHistory)?;
        let expected_ref = format!("refs/heads/{}", self.plan.head_ref);
        let matches = receipt.reference == expected_ref
            && receipt.object_type == "commit"
            && receipt.sha == commit_sha;
        let event = if matches {
            TransitionEvent::ReferencePublished { receipt }
        } else {
            TransitionEvent::EffectUncertain {
                effect: TransitionEffect::PublishReference,
                reason: UncertaintyReason::MismatchedResponse,
                observed_response: Some(ObservedResponse::ReferenceUpdate(receipt)),
            }
        };
        self.append(event)?;
        Ok(disposition(matches))
    }

    /// Record a transport or decode failure after publication was claimed.
    pub(crate) fn publication_uncertain(
        &mut self,
        reason: UncertaintyReason,
    ) -> Result<(), TransitionError> {
        self.mark_uncertain(TransitionEffect::PublishReference, reason)
    }

    fn latest_commit_sha(&self) -> Option<&str> {
        self.events.iter().find_map(|event| match event {
            TransitionEvent::NonceCommitRecorded { receipt } => Some(receipt.sha.as_str()),
            _ => None,
        })
    }

    fn mark_uncertain(
        &mut self,
        effect: TransitionEffect,
        reason: UncertaintyReason,
    ) -> Result<(), TransitionError> {
        self.append(TransitionEvent::EffectUncertain {
            effect,
            reason,
            observed_response: None,
        })
    }

    fn append(&mut self, event: TransitionEvent) -> Result<(), TransitionError> {
        self.apply(event)
    }

    fn apply(&mut self, event: TransitionEvent) -> Result<(), TransitionError> {
        let valid = self.event_is_valid(&event);
        if !valid {
            return Err(TransitionError::IncorrectHistory);
        }
        self.events.push(event);
        Ok(())
    }

    fn event_is_valid(&self, event: &TransitionEvent) -> bool {
        match (event, self.stage()) {
            (TransitionEvent::RetargetClaimed, TransitionStage::Prepared) => true,
            (
                TransitionEvent::RetargetResponseObserved { receipt },
                TransitionStage::RetargetInFlight,
            ) => receipt.matches(&self.plan),
            (
                TransitionEvent::RetargetAcknowledged { receipt },
                TransitionStage::RetargetResponseObserved,
            ) => self
                .retarget_response()
                .is_some_and(|response| receipt.matches(&self.plan, response)),
            (TransitionEvent::NonceReserved { nonce }, TransitionStage::Retargeted) => {
                valid_hex(nonce, NONCE_BYTES * 2)
            }
            (TransitionEvent::NonceCommitClaimed, TransitionStage::NonceReserved) => true,
            (
                TransitionEvent::NonceCommitRecorded { receipt },
                TransitionStage::NonceCommitInFlight,
            ) => self.commit_receipt_matches(receipt),
            (TransitionEvent::ReferenceUpdateClaimed, TransitionStage::NonceCommitCreated) => true,
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
                    && uncertainty_reason_matches_observation(*reason, observed_response.is_some())
            }
            _ => false,
        }
    }

    fn commit_receipt_matches(&self, receipt: &NonceCommitReceipt) -> bool {
        self.nonce().is_some_and(|nonce| {
            receipt.parent_sha == self.plan.head_sha
                && valid_sha(&receipt.sha)
                && valid_sha(&receipt.tree_sha)
                && valid_hex(&receipt.message_sha256, 64)
                && receipt.nonce_marker_count == 1
                && receipt.sign_off_count == 1
                && receipt.author_name == self.plan.commit_identity.name()
                && receipt.author_email == self.plan.commit_identity.email()
                && receipt.committer_name == self.plan.commit_identity.name()
                && receipt.committer_email == self.plan.commit_identity.email()
                && !nonce.is_empty()
        })
    }

    fn retarget_response(&self) -> Option<&RetargetResponseReceipt> {
        self.events.iter().find_map(|event| match event {
            TransitionEvent::RetargetResponseObserved { receipt }
            | TransitionEvent::RetargetAcknowledged {
                response: receipt, ..
            } => Some(receipt),
            _ => None,
        })
    }

    fn reference_matches(&self, receipt: &ReferenceReceipt) -> bool {
        self.latest_commit_sha().is_some_and(|sha| {
            receipt.reference == format!("refs/heads/{}", self.plan.head_ref)
                && receipt.object_type == "commit"
                && receipt.sha == sha
        })
    }
}

struct Nonce(String);

impl Nonce {
    fn generate() -> Result<Self, TransitionError> {
        let mut bytes = [0_u8; NONCE_BYTES];
        fill(&mut bytes).map_err(|_| TransitionError::RandomSource)?;
        Ok(Self(hex(&bytes)))
    }
}

fn disposition(recorded: bool) -> ResponseDisposition {
    if recorded {
        ResponseDisposition::Recorded
    } else {
        ResponseDisposition::Uncertain
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
            None | Some(ObservedResponse::Retarget(_))
        ) | (
            TransitionEffect::Retarget,
            Some(ObservedResponse::RetargetHistory(_))
        ) | (
            TransitionEffect::NonceCommit,
            None | Some(ObservedResponse::NonceCommit(_))
        ) | (
            TransitionEffect::PublishReference,
            None | Some(ObservedResponse::ReferenceUpdate(_))
        )
    )
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
        | UncertaintyReason::RestartedInFlight => true,
    }
}
