//! Durable one-shot journal boundaries for the pull-request source transition.
//!
//! This component records effect claims before a caller invokes the existing GitHub helpers. It
//! does not perform remote operations. Loading a transition only validates its history; after a
//! restart, callers must explicitly recover before they can obtain another capability.

mod capability;
mod journal_api;
mod model;
mod storage;
#[cfg(test)]
mod storage_tests;

use crate::error::HostError;

use capability::effect_claim;
pub use capability::{
    EffectClaim, PrSourceTransitionJournal, TransitionRecovery, TransitionSnapshot,
};
use model::{RecoveryAction, TransitionLedger};
use model::{ReferenceReceipt, RetargetHistoryReceipt, RetargetResponseReceipt};
pub use model::{
    ResponseDisposition as TransitionResponseDisposition, TransitionEffect, TransitionError,
    TransitionPlan, TransitionPlanInput, TransitionStage, UncertaintyReason,
};
pub(crate) use storage::validate_persisted_rows;

impl PrSourceTransitionJournal<'_> {
    async fn load_ledger(&self, transition_id: &str) -> Result<TransitionLedger, HostError> {
        storage::load(self.journal, transition_id).await
    }

    fn expect_version(
        ledger: &TransitionLedger,
        expected_plan_digest: &str,
        expected_event_count: usize,
    ) -> Result<(), HostError> {
        if ledger.events().len() == expected_event_count
            && ledger.plan().digest().as_deref() == Ok(expected_plan_digest)
        {
            Ok(())
        } else {
            Err(HostError::Journal)
        }
    }

    async fn persist_last_event(
        &self,
        transition_id: &str,
        expected_plan_digest: &str,
        expected_event_count: usize,
        ledger: TransitionLedger,
    ) -> Result<TransitionLedger, HostError> {
        let event = ledger.events().last().ok_or(HostError::Journal)?.clone();
        storage::append(
            self.journal,
            transition_id,
            expected_plan_digest,
            expected_event_count,
            event,
        )
        .await
    }

    async fn persist_snapshot(
        &self,
        transition_id: &str,
        expected_plan_digest: &str,
        expected_event_count: usize,
        ledger: TransitionLedger,
    ) -> Result<TransitionSnapshot, HostError> {
        let ledger = self
            .persist_last_event(
                transition_id,
                expected_plan_digest,
                expected_event_count,
                ledger,
            )
            .await?;
        Ok(TransitionSnapshot { ledger })
    }
}
