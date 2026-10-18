//! Strict writer result consumed as an untrusted claim by the merge gate.

use serde::{Deserialize, Serialize};

/// Durable promotion receipt emitted after opening a PR or reconciling a prior merge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TapPromotionReceipt {
    /// Versioned result schema (`velnor.homebrew-tap-pr.v2`).
    pub schema: String,
    /// Target tap repository.
    pub repository: String,
    /// Authenticated source release receipt digest.
    pub release_receipt_digest: String,
    /// Digest of the current preparation proof.
    pub preparation_proof_digest: String,
    /// Selected open PR or authenticated historical merged-PR candidate.
    pub outcome: TapPromotionOutcome,
}

/// Tagged writer result. Merge independently rechecks all state and identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum TapPromotionOutcome {
    /// Current open PR created or reconciled by the writer.
    PullRequest {
        /// Pull request number.
        pull_request: u64,
        /// Exact PR head commit SHA.
        head_sha: String,
        /// Captured tap-main commit SHA.
        base_sha: String,
        /// Deterministic branch derived from receipt and base.
        branch: String,
        /// Lowercase SHA-256 of the changed preparation proof.
        candidate_proof_digest: String,
    },
    /// Historical merged PR candidate authenticated by the writer.
    AlreadyPromoted {
        /// Pull request number.
        pull_request: u64,
        /// Exact historical PR head commit SHA.
        head_sha: String,
        /// Original tap-main parent of the historical candidate commit.
        base_sha: String,
        /// Historical branch derived from receipt and original base.
        branch: String,
        /// Lowercase SHA-256 of the reconstructed changed proof.
        candidate_proof_digest: String,
        /// Squash merge commit SHA, checked against the PR tree and original base.
        merge_sha: String,
        /// GitHub merge timestamp for the selected historical PR.
        merged_at: String,
    },
}
