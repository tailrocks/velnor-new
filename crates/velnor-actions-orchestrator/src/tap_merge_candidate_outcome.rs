// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    HandoffValidation, OrchestratorError, TapPreparationDisposition, TapPromotionOutcome,
    expected_branch, internal, is_hex,
};

pub(super) fn validate_promotion_outcome(
    input: &HandoffValidation<'_>,
) -> Result<(), OrchestratorError> {
    let HandoffValidation {
        proof,
        promotion,
        release,
        ..
    } = input;
    match &promotion.outcome {
        TapPromotionOutcome::PullRequest {
            pull_request,
            head_sha,
            base_sha,
            branch,
            candidate_proof_digest,
        } if proof.disposition == TapPreparationDisposition::Changed
            && *pull_request > 0
            && base_sha == &proof.base_sha
            && branch
                == &expected_branch(
                    &release.receipt_digest,
                    base_sha,
                    release.receipt.channel,
                )?
            && candidate_proof_digest == &promotion.preparation_proof_digest
            && is_hex(head_sha, 40)
            && is_hex(base_sha, 40) => {}
        TapPromotionOutcome::AlreadyPromoted {
            pull_request,
            head_sha,
            base_sha,
            branch,
            candidate_proof_digest,
            merge_sha,
            merged_at,
        } if proof.disposition == TapPreparationDisposition::FormulaAlreadyCurrent
            && proof.old_formula_sha256 == proof.new_formula_sha256
            && *pull_request > 0
            && branch
                == &expected_branch(
                    &release.receipt_digest,
                    base_sha,
                    release.receipt.channel,
                )?
            && is_hex(candidate_proof_digest, 64)
            && is_hex(head_sha, 40)
            && is_hex(base_sha, 40)
            && is_hex(merge_sha, 40)
            && !merged_at.is_empty() => {}
        TapPromotionOutcome::InitializedByCutover {
            cutover_pull_request,
            head_sha,
            base_sha,
            merge_sha,
            merged_at,
            marker_sha256,
        } if proof.disposition == TapPreparationDisposition::FormulaAlreadyCurrent
            && proof.old_formula_sha256 == proof.new_formula_sha256
            && release.receipt.channel
                == velnor_actions_contract::package_release::PackageReleaseChannel::Preview
            && *cutover_pull_request > 0
            && is_hex(head_sha, 40)
            && is_hex(base_sha, 40)
            && is_hex(merge_sha, 40)
            && is_hex(marker_sha256, 64)
            && !merged_at.is_empty() => {}
        TapPromotionOutcome::SkippedStaleSource { .. } => {
            return Err(internal("tap_merge_skipped_promotion_invalid"));
        }
        _ => return Err(internal("tap_merge_handoff_binding_mismatch")),
    }
    Ok(())
}
