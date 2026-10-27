// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    BotIdentity, OrchestratorError, TapPromotionOutcome, TapPromotionReceipt, WriterInputs,
    gitops::{
        BranchRequest, CurrentSourceWrite, PullRequestRequest, create_or_reconcile_branch,
        create_or_reconcile_pr,
    },
    helpers::{self, branch_name, read_formula_base, sha256, verify_repository_base},
    identity::verify_app_scope,
    internal, persist_result, verify_promotion_authority,
};
use velnor_actions_contract::canonical_json_bytes;

pub(super) enum WriterPreparation {
    Stale(TapPromotionReceipt),
    Ready {
        bot: BotIdentity,
        base_sha: String,
        base_tree: helpers::FormulaBase,
        proof_digest: String,
    },
}

pub(super) fn prepare_writer(
    inputs: &WriterInputs,
) -> Result<WriterPreparation, OrchestratorError> {
    let bot = verify_app_scope(
        &inputs.api,
        &inputs.policy,
        inputs.app_id,
        inputs.installation_id,
        &inputs.app_slug,
    )?;
    let base_sha = verify_repository_base(&inputs.api, &inputs.policy, &inputs.proof)?;
    let base_tree = read_formula_base(&inputs.api, &inputs.formula_path, &base_sha)?;
    if sha256(&base_tree.bytes) != inputs.proof.old_formula_sha256 {
        return Err(internal("tap_candidate_base_digest_mismatch"));
    }
    let canonical =
        crate::tap_prepare::derive_formula_candidate(crate::tap_prepare::FormulaCandidateInput {
            spec: inputs.spec.as_ref(),
            source_repository: &inputs.proof.source_repository,
            tap_repository: &inputs.policy.repository,
            formula_path: &inputs.formula_path,
            base_sha: &base_sha,
            base_formula: &base_tree.bytes,
            proposed_formula: Some(&inputs.proposed),
            plan: &inputs.plan,
            release: &inputs.release,
        })
        .map_err(|_| internal("tap_writer_formula_rejected"))?;
    if canonical != inputs.proposed {
        return Err(internal("tap_writer_formula_noncanonical"));
    }
    let preparation_proof_digest =
        sha256(&canonical_json_bytes(&inputs.proof).map_err(|_| internal("tap_proof_encode"))?);
    if inputs.plan.channel
        == velnor_actions_contract::package_release::PackageReleaseChannel::Preview
        && !crate::tap_source_freshness::verify_preview_source_is_current(
            &inputs.plan.repository,
            &inputs.plan.default_branch,
            inputs.release.receipt.repository.id,
            &inputs.release.receipt.source.commit_sha,
        )?
    {
        return Ok(WriterPreparation::Stale(stale_source_receipt(
            inputs,
            &preparation_proof_digest,
        )));
    }
    Ok(WriterPreparation::Ready {
        bot,
        base_sha,
        base_tree,
        proof_digest: preparation_proof_digest,
    })
}

pub(super) fn create_pull_request(
    inputs: &WriterInputs,
    bot: &BotIdentity,
    base_sha: &str,
    base_tree_sha: &str,
    proof_digest: &str,
) -> Result<TapPromotionReceipt, OrchestratorError> {
    let branch = branch_name(
        &inputs.release.receipt_digest,
        base_sha,
        inputs.plan.channel,
    )?;
    let mut verify_current = || verify_promotion_authority(inputs);
    let head_sha = match create_or_reconcile_branch(
        &inputs.api,
        &mut BranchRequest {
            proof: &inputs.proof,
            formula: &inputs.proposed,
            base_sha,
            base_tree_sha,
            branch: &branch,
            bot,
            verify_current: &mut verify_current,
        },
    )? {
        CurrentSourceWrite::Completed(head_sha) => head_sha,
        CurrentSourceWrite::SkippedStaleSource => {
            return Ok(stale_source_receipt(inputs, proof_digest));
        }
    };
    let pull_request = match create_or_reconcile_pr(
        &inputs.api,
        &mut PullRequestRequest {
            policy: &inputs.policy,
            branch: &branch,
            head_sha: &head_sha,
            base_sha,
            proof_digest,
            release: &inputs.release,
            bot,
            verify_current: &mut verify_current,
        },
    )? {
        CurrentSourceWrite::Completed(pull_request) => pull_request,
        CurrentSourceWrite::SkippedStaleSource => {
            return Ok(stale_source_receipt(inputs, proof_digest));
        }
    };
    let result = TapPromotionReceipt {
        schema: "velnor.homebrew-tap-pr.v3".to_owned(),
        repository: inputs.policy.repository.clone(),
        release_receipt_digest: inputs.release.receipt_digest.clone(),
        preparation_proof_digest: proof_digest.to_owned(),
        outcome: TapPromotionOutcome::PullRequest {
            pull_request,
            head_sha,
            base_sha: base_sha.to_owned(),
            branch,
            candidate_proof_digest: proof_digest.to_owned(),
        },
    };
    persist_result(&inputs.root, &result)?;
    Ok(result)
}

fn stale_source_receipt(inputs: &WriterInputs, proof_digest: &str) -> TapPromotionReceipt {
    TapPromotionReceipt {
        schema: "velnor.homebrew-tap-pr.v3".to_owned(),
        repository: inputs.policy.repository.clone(),
        release_receipt_digest: inputs.release.receipt_digest.clone(),
        preparation_proof_digest: proof_digest.to_owned(),
        outcome: TapPromotionOutcome::SkippedStaleSource {
            source_sha: inputs.release.receipt.source.commit_sha.clone(),
        },
    }
}
