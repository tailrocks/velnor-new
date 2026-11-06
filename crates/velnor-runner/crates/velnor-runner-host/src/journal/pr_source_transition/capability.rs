// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;
use crate::journal::Journal;
use velnor_runner_github::workflow_run::PullRequestNonceCommitReceipt;

use super::model::{NonceCommitReceipt, TransitionLedger};
use super::{TransitionEffect, TransitionPlan, TransitionStage};

pub struct PrSourceTransitionJournal<'a> {
    pub(super) journal: &'a Journal,
}

impl std::fmt::Debug for PrSourceTransitionJournal<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PrSourceTransitionJournal")
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionSnapshot {
    pub(super) ledger: TransitionLedger,
}

impl TransitionSnapshot {
    #[must_use]
    pub const fn plan(&self) -> &TransitionPlan {
        self.ledger.plan()
    }

    pub fn transition_id(&self) -> Result<String, HostError> {
        self.ledger
            .plan()
            .transition_id()
            .map_err(|_| HostError::Journal)
    }

    pub fn plan_digest(&self) -> Result<String, HostError> {
        self.ledger.plan().digest().map_err(|_| HostError::Journal)
    }

    #[must_use]
    pub fn stage(&self) -> TransitionStage {
        self.ledger.stage()
    }

    #[must_use]
    pub fn event_count(&self) -> usize {
        self.ledger.events().len()
    }

    #[must_use]
    pub fn nonce_commit_receipt(&self) -> Option<PullRequestNonceCommitReceipt> {
        if self.ledger.stage() != TransitionStage::NonceCommitCreated {
            return None;
        }
        let nonce = self.ledger.nonce()?;
        let receipt: &NonceCommitReceipt = self.ledger.nonce_commit_receipt()?;
        Some(receipt.helper_receipt(self.ledger.plan(), nonce))
    }
}

#[must_use = "effect claims must be used only for their one corresponding GitHub helper call"]
#[derive(Debug)]
pub struct EffectClaim {
    pub(super) transition_id: String,
    pub(super) plan_digest: String,
    pub(super) event_count: usize,
    pub(super) effect: TransitionEffect,
    pub(super) nonce: Option<String>,
}

impl EffectClaim {
    #[must_use]
    pub fn transition_id(&self) -> &str {
        &self.transition_id
    }

    #[must_use]
    pub fn plan_digest(&self) -> &str {
        &self.plan_digest
    }

    #[must_use]
    pub const fn event_count(&self) -> usize {
        self.event_count
    }

    #[must_use]
    pub const fn effect(&self) -> TransitionEffect {
        self.effect
    }

    #[must_use]
    pub fn nonce(&self) -> Option<&str> {
        self.nonce.as_deref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionRecovery {
    NoInterruptedEffect,

    RecordedUncertainty,

    VerifyRetargetHistoryReadOnly,
}

pub(super) fn effect_claim(
    transition_id: &str,
    ledger: &TransitionLedger,
    effect: TransitionEffect,
    nonce: Option<String>,
) -> Result<EffectClaim, HostError> {
    Ok(EffectClaim {
        transition_id: transition_id.to_owned(),
        plan_digest: ledger.plan().digest().map_err(|_| HostError::Journal)?,
        event_count: ledger.events().len(),
        effect,
        nonce,
    })
}
