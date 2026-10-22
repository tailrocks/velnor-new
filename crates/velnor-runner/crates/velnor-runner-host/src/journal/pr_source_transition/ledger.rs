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
use super::validation::{NONCE_BYTES, hex};

#[path = "ledger/validation.rs"]
mod validation;

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

    /// Apply one candidate event before persisting it in the journal transaction.
    pub(crate) fn append_validated(
        &mut self,
        event: TransitionEvent,
    ) -> Result<(), TransitionError> {
        self.append(event)
    }

    /// Mark an interrupted claimed effect uncertain, or resume only the read-only timeline check.
    pub(crate) fn recover_after_restart(&mut self) -> Result<RecoveryAction, TransitionError> {
        match self.stage() {
            TransitionStage::RetargetInFlight => {
                self.retarget_uncertain(UncertaintyReason::RestartedInFlight)?;
                Ok(RecoveryAction::RecordedUncertainty)
            }
            TransitionStage::NonceCommitInFlight => {
                self.nonce_commit_uncertain(UncertaintyReason::RestartedInFlight)?;
                Ok(RecoveryAction::RecordedUncertainty)
            }
            TransitionStage::ReferenceUpdateInFlight => {
                self.publication_uncertain(UncertaintyReason::RestartedInFlight)?;
                Ok(RecoveryAction::RecordedUncertainty)
            }
            TransitionStage::RetargetResponseObserved => {
                Ok(RecoveryAction::VerifyRetargetHistoryReadOnly)
            }
            _ => Ok(RecoveryAction::NoInterruptedEffect),
        }
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
        } else if receipt.is_well_formed() {
            TransitionEvent::EffectUncertain {
                effect: TransitionEffect::Retarget,
                reason: UncertaintyReason::MismatchedResponse,
                observed_response: Some(ObservedResponse::Retarget(receipt)),
            }
        } else {
            TransitionEvent::EffectUncertain {
                effect: TransitionEffect::Retarget,
                reason: UncertaintyReason::MalformedResponse,
                observed_response: None,
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

    pub(crate) fn nonce_commit_receipt(&self) -> Option<&NonceCommitReceipt> {
        self.events.iter().find_map(|event| match event {
            TransitionEvent::NonceCommitRecorded { receipt } => Some(receipt),
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
            None => match NonceCommitObservation::from_commit(commit) {
                Ok(observation) => TransitionEvent::EffectUncertain {
                    effect: TransitionEffect::NonceCommit,
                    reason: UncertaintyReason::MismatchedResponse,
                    observed_response: Some(ObservedResponse::NonceCommit(observation)),
                },
                Err(_) => TransitionEvent::EffectUncertain {
                    effect: TransitionEffect::NonceCommit,
                    reason: UncertaintyReason::MalformedResponse,
                    observed_response: None,
                },
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
        if !receipt.is_well_formed() {
            self.publication_uncertain(UncertaintyReason::MalformedResponse)?;
            return Ok(ResponseDisposition::Uncertain);
        }
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

    pub(crate) fn nonce_commit_mismatched(
        &mut self,
        commit: &GitCommit,
    ) -> Result<(), TransitionError> {
        match NonceCommitObservation::from_commit(commit) {
            Ok(observation) => self.append(TransitionEvent::EffectUncertain {
                effect: TransitionEffect::NonceCommit,
                reason: UncertaintyReason::MismatchedResponse,
                observed_response: Some(ObservedResponse::NonceCommit(observation)),
            }),
            Err(_) => self.append(TransitionEvent::EffectUncertain {
                effect: TransitionEffect::NonceCommit,
                reason: UncertaintyReason::MalformedResponse,
                observed_response: None,
            }),
        }
    }

    pub(crate) fn mark_uncertain(
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
        if self.events.len() >= 16 {
            return Err(TransitionError::IncorrectHistory);
        }
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

    pub(crate) fn retarget_response(&self) -> Option<&RetargetResponseReceipt> {
        self.events.iter().find_map(|event| match event {
            TransitionEvent::RetargetResponseObserved { receipt }
            | TransitionEvent::RetargetAcknowledged {
                response: receipt, ..
            } => Some(receipt),
            _ => None,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryAction {
    NoInterruptedEffect,
    RecordedUncertainty,
    VerifyRetargetHistoryReadOnly,
}
