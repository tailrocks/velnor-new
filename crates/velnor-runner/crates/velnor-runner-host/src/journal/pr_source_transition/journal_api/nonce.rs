use crate::error::HostError;
use velnor_runner_github::workflow_run::PullRequestNonceCommitReceipt;

use super::super::{
    EffectClaim, PrSourceTransitionJournal, TransitionEffect, TransitionSnapshot, effect_claim,
};

impl PrSourceTransitionJournal<'_> {
    /// Generate and durably reserve a random nonce after retarget acknowledgement.
    ///
    /// # Errors
    ///
    /// Returns an error if retargeting is not acknowledged, the expected version is stale, or
    /// the nonce cannot be persisted.
    pub async fn reserve_nonce(
        &self,
        transition_id: &str,
        expected_plan_digest: &str,
        expected_event_count: usize,
    ) -> Result<String, HostError> {
        let mut ledger = self.load_ledger(transition_id).await?;
        Self::expect_version(&ledger, expected_plan_digest, expected_event_count)?;
        let nonce = ledger.reserve_nonce().map_err(|_| HostError::Journal)?;
        self.persist_last_event(
            transition_id,
            expected_plan_digest,
            expected_event_count,
            ledger,
        )
        .await?;
        Ok(nonce)
    }

    /// Persist the one-shot commit-creation claim before calling the nonce-commit helper.
    ///
    /// # Errors
    ///
    /// Returns an error if the nonce is not durably reserved, the expected version is stale, or
    /// the claim cannot be persisted.
    pub async fn claim_nonce_commit(
        &self,
        transition_id: &str,
        expected_plan_digest: &str,
        expected_event_count: usize,
    ) -> Result<EffectClaim, HostError> {
        let mut ledger = self.load_ledger(transition_id).await?;
        Self::expect_version(&ledger, expected_plan_digest, expected_event_count)?;
        let nonce = ledger.nonce().ok_or(HostError::Journal)?.to_owned();
        ledger
            .claim_nonce_commit()
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
            TransitionEffect::NonceCommit,
            Some(nonce),
        )
    }

    /// Record the validated helper response, dropping its unbounded message before persistence.
    ///
    /// # Errors
    ///
    /// Returns an error if the expected version is stale or the result cannot be persisted.
    pub async fn record_nonce_commit(
        &self,
        transition_id: &str,
        expected_plan_digest: &str,
        expected_event_count: usize,
        receipt: &PullRequestNonceCommitReceipt,
    ) -> Result<TransitionSnapshot, HostError> {
        let mut ledger = self.load_ledger(transition_id).await?;
        Self::expect_version(&ledger, expected_plan_digest, expected_event_count)?;
        let wrapper_matches = receipt.head_repository_id == ledger.plan().head_repository_id
            && receipt.head_ref == ledger.plan().head_ref
            && receipt.parent_sha == ledger.plan().head_sha
            && Some(receipt.nonce.as_str()) == ledger.nonce()
            && receipt.identity == ledger.plan().commit_identity;
        if wrapper_matches {
            ledger
                .record_nonce_commit(&receipt.commit)
                .map_err(|_| HostError::Journal)?;
        } else {
            ledger
                .nonce_commit_mismatched(&receipt.commit)
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
}
