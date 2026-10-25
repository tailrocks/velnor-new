use crate::error::HostError;
use velnor_runner_github::workflow_run::GitReference;

use super::super::{
    EffectClaim, PrSourceTransitionJournal, RecoveryAction, ReferenceReceipt, TransitionEffect,
    TransitionRecovery, TransitionSnapshot, UncertaintyReason, effect_claim,
};

impl PrSourceTransitionJournal<'_> {
    /// Persist the non-force reference-update claim before calling the publication helper.
    ///
    /// # Errors
    ///
    /// Returns an error if the nonce commit is not recorded, the expected version is stale, or
    /// the claim cannot be persisted.
    pub async fn claim_reference_update(
        &self,
        transition_id: &str,
        expected_plan_digest: &str,
        expected_event_count: usize,
    ) -> Result<EffectClaim, HostError> {
        let mut ledger = self.load_ledger(transition_id).await?;
        Self::expect_version(&ledger, expected_plan_digest, expected_event_count)?;
        ledger
            .claim_reference_update()
            .map_err(|_| HostError::Journal)?;
        let ledger = self
            .persist_last_event(
                transition_id,
                expected_plan_digest,
                expected_event_count,
                ledger,
            )
            .await?;
        effect_claim(
            transition_id,
            &ledger,
            TransitionEffect::PublishReference,
            None,
        )
    }

    /// Record the exact reference receipt after one successful non-force update.
    ///
    /// # Errors
    ///
    /// Returns an error if the expected version is stale or the result cannot be persisted.
    pub async fn record_reference_update(
        &self,
        transition_id: &str,
        expected_plan_digest: &str,
        expected_event_count: usize,
        reference: &GitReference,
    ) -> Result<TransitionSnapshot, HostError> {
        let mut ledger = self.load_ledger(transition_id).await?;
        Self::expect_version(&ledger, expected_plan_digest, expected_event_count)?;
        let fields_bounded = reference.reference.len() <= 512
            && reference.object.object_type.len() <= 64
            && reference.object.sha.len() <= 64;
        if fields_bounded {
            ledger
                .record_reference_update(ReferenceReceipt {
                    reference: reference.reference.clone(),
                    object_type: reference.object.object_type.clone(),
                    sha: reference.object.sha.clone(),
                })
                .map_err(|_| HostError::Journal)?;
        } else {
            ledger
                .publication_uncertain(UncertaintyReason::MalformedResponse)
                .map_err(|_| HostError::Journal)?;
        }
        self.persist_snapshot(
            transition_id,
            expected_plan_digest,
            expected_event_count,
            ledger,
        )
        .await
    }

    /// Durably record a bounded failure after an already-persisted effect claim.
    ///
    /// # Errors
    ///
    /// Returns an error if the expected version is stale, the effect does not match the current
    /// claim, or uncertainty cannot be persisted.
    pub async fn record_uncertain(
        &self,
        transition_id: &str,
        expected_plan_digest: &str,
        expected_event_count: usize,
        effect: TransitionEffect,
        reason: UncertaintyReason,
    ) -> Result<TransitionSnapshot, HostError> {
        if matches!(
            reason,
            UncertaintyReason::MismatchedResponse | UncertaintyReason::RestartedInFlight
        ) {
            return Err(HostError::Journal);
        }
        let mut ledger = self.load_ledger(transition_id).await?;
        Self::expect_version(&ledger, expected_plan_digest, expected_event_count)?;
        ledger
            .mark_uncertain(effect, reason)
            .map_err(|_| HostError::Journal)?;
        self.persist_snapshot(
            transition_id,
            expected_plan_digest,
            expected_event_count,
            ledger,
        )
        .await
    }

    /// Explicitly close uncertain in-flight claims after restart. This never returns an effect
    /// capability; an observed retarget response can continue only through the history API above.
    ///
    /// # Errors
    ///
    /// Returns an error if the expected version is stale or recovery cannot be persisted.
    pub async fn recover_after_restart(
        &self,
        transition_id: &str,
        expected_plan_digest: &str,
        expected_event_count: usize,
    ) -> Result<TransitionRecovery, HostError> {
        let mut ledger = self.load_ledger(transition_id).await?;
        Self::expect_version(&ledger, expected_plan_digest, expected_event_count)?;
        let action = ledger
            .recover_after_restart()
            .map_err(|_| HostError::Journal)?;
        let action = match action {
            RecoveryAction::NoInterruptedEffect => TransitionRecovery::NoInterruptedEffect,
            RecoveryAction::VerifyRetargetHistoryReadOnly => {
                TransitionRecovery::VerifyRetargetHistoryReadOnly
            }
            RecoveryAction::RecordedUncertainty => {
                self.persist_snapshot(
                    transition_id,
                    expected_plan_digest,
                    expected_event_count,
                    ledger,
                )
                .await?;
                TransitionRecovery::RecordedUncertainty
            }
        };
        Ok(action)
    }
}
