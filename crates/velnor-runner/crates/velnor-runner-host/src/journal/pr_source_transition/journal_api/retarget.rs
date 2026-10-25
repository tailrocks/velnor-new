use crate::error::HostError;
use velnor_runner_github::workflow_run::{PullRequestBaseHistory, PullRequestBaseUpdateReceipt};

use super::super::{
    EffectClaim, PrSourceTransitionJournal, RetargetHistoryReceipt, RetargetResponseReceipt,
    TransitionEffect, TransitionPlan, TransitionSnapshot, UncertaintyReason, effect_claim, storage,
};

impl PrSourceTransitionJournal<'_> {
    /// Start one immutable transition plan, or reopen the exact same already-started plan.
    ///
    /// # Errors
    ///
    /// Returns an error if the plan conflicts with an existing transition or persistence fails.
    pub async fn begin(&self, plan: TransitionPlan) -> Result<TransitionSnapshot, HostError> {
        storage::begin(self.journal, plan)
            .await
            .map(|ledger| TransitionSnapshot { ledger })
    }

    /// Load and strictly replay one transition. This does not authorize any effect.
    ///
    /// # Errors
    ///
    /// Returns an error if the transition is missing, corrupt or cannot be read.
    pub async fn load(&self, transition_id: &str) -> Result<TransitionSnapshot, HostError> {
        storage::load(self.journal, transition_id)
            .await
            .map(|ledger| TransitionSnapshot { ledger })
    }

    /// Persist the one-shot PATCH claim before calling the retarget helper.
    ///
    /// # Errors
    ///
    /// Returns an error if the expected version is stale, the transition is not prepared, or
    /// the claim cannot be persisted.
    pub async fn claim_retarget(
        &self,
        transition_id: &str,
        expected_plan_digest: &str,
        expected_event_count: usize,
    ) -> Result<EffectClaim, HostError> {
        let mut ledger = self.load_ledger(transition_id).await?;
        Self::expect_version(&ledger, expected_plan_digest, expected_event_count)?;
        ledger.claim_retarget().map_err(|_| HostError::Journal)?;
        let ledger = self
            .persist_last_event(
                transition_id,
                expected_plan_digest,
                expected_event_count,
                ledger,
            )
            .await?;
        effect_claim(transition_id, &ledger, TransitionEffect::Retarget, None)
    }

    /// Persist the exact base-update response before reading the full timeline.
    ///
    /// # Errors
    ///
    /// Returns an error if the expected version is stale, the response is invalid for this
    /// transition, or persistence fails.
    pub async fn record_retarget_response(
        &self,
        transition_id: &str,
        expected_plan_digest: &str,
        expected_event_count: usize,
        receipt: &PullRequestBaseUpdateReceipt,
    ) -> Result<TransitionSnapshot, HostError> {
        let mut ledger = self.load_ledger(transition_id).await?;
        Self::expect_version(&ledger, expected_plan_digest, expected_event_count)?;
        let response = RetargetResponseReceipt::from_update_receipt(receipt);
        ledger
            .record_retarget_response(response)
            .map_err(|_| HostError::Journal)?;
        self.persist_snapshot(
            transition_id,
            expected_plan_digest,
            expected_event_count,
            ledger,
        )
        .await
    }

    /// Record the exact one-event retarget proof from a complete read-only timeline snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if the expected version is stale or the result cannot be persisted.
    pub async fn record_retarget_verification(
        &self,
        transition_id: &str,
        expected_plan_digest: &str,
        expected_event_count: usize,
        history: &PullRequestBaseHistory,
    ) -> Result<TransitionSnapshot, HostError> {
        let mut ledger = self.load_ledger(transition_id).await?;
        Self::expect_version(&ledger, expected_plan_digest, expected_event_count)?;
        let response = ledger
            .retarget_response()
            .ok_or(HostError::Journal)?
            .clone();
        match RetargetHistoryReceipt::from_history(history, ledger.plan(), &response) {
            Ok(receipt) => {
                ledger
                    .record_retarget_verification(receipt)
                    .map_err(|_| HostError::Journal)?;
            }
            Err(_) => ledger
                .mark_uncertain(
                    TransitionEffect::Retarget,
                    UncertaintyReason::PostEffectHistoryCheck,
                )
                .map_err(|_| HostError::Journal)?,
        }
        self.persist_snapshot(
            transition_id,
            expected_plan_digest,
            expected_event_count,
            ledger,
        )
        .await
    }
}
