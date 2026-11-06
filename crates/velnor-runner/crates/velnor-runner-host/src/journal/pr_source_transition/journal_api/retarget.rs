// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;
use velnor_runner_github::workflow_run::{PullRequestBaseHistory, PullRequestBaseUpdateReceipt};

use super::super::{
    EffectClaim, PrSourceTransitionJournal, RetargetHistoryReceipt, RetargetResponseReceipt,
    TransitionEffect, TransitionPlan, TransitionSnapshot, UncertaintyReason, effect_claim, storage,
};

impl PrSourceTransitionJournal<'_> {
    pub async fn begin(&self, plan: TransitionPlan) -> Result<TransitionSnapshot, HostError> {
        storage::begin(self.journal, plan)
            .await
            .map(|ledger| TransitionSnapshot { ledger })
    }

    pub async fn load(&self, transition_id: &str) -> Result<TransitionSnapshot, HostError> {
        storage::load(self.journal, transition_id)
            .await
            .map(|ledger| TransitionSnapshot { ledger })
    }

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
