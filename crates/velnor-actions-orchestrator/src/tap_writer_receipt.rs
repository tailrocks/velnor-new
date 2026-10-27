// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TapPromotionReceipt {
    pub schema: String,
    pub repository: String,
    pub release_receipt_digest: String,
    pub preparation_proof_digest: String,
    pub outcome: TapPromotionOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum TapPromotionOutcome {
    PullRequest {
        pull_request: u64,
        head_sha: String,
        base_sha: String,
        branch: String,
        candidate_proof_digest: String,
    },
    AlreadyPromoted {
        pull_request: u64,
        head_sha: String,
        base_sha: String,
        branch: String,
        candidate_proof_digest: String,
        merge_sha: String,
        merged_at: String,
    },
    InitializedByCutover {
        cutover_pull_request: u64,
        head_sha: String,
        base_sha: String,
        merge_sha: String,
        merged_at: String,
        marker_sha256: String,
    },
    SkippedStaleSource {
        source_sha: String,
    },
}
