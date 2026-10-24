use super::super::{candidate::MergeCandidate, observation::MergeAuthority};
use super::response_helpers::sha256;
use super::responses::completed_responses;
use crate::{
    homebrew::test_support as tap_test_support,
    tap_prepare::{
        HomebrewTapPreparationProof, TapPreparationDisposition, TapPromotionOutcome,
        TapPromotionReceipt,
    },
};
use serde_json::Value;
use std::collections::BTreeMap;
use velnor_actions_contract::canonical_json_bytes;

const PR: u64 = 21;
const WRITER_APP: u64 = 17;
const COMPLETION_APP: u64 = 19;
const REVIEWER_APP: u64 = 23;

pub(super) fn completed_case() -> (
    MergeCandidate,
    MergeAuthority,
    String,
    BTreeMap<String, Value>,
) {
    let fixture = tap_test_support::fixture();
    let base = "b".repeat(40);
    let head = "c".repeat(40);
    let merge = "d".repeat(40);
    let branch = super::super::candidate::expected_branch(
        &fixture.release.receipt_digest,
        &base,
        fixture.release.receipt.channel,
    )
    .expect("receipt and base branch");
    let historical = crate::tap_prepare::reconstruct_historical_candidate_proof(
        crate::tap_prepare::HistoricalCandidateInput {
            source_repository: &fixture.plan.repository,
            tap: &fixture.policy,
            plan: &fixture.plan,
            release: &fixture.release,
            spec: &fixture.spec,
            sdk_artifact_digest: &"9".repeat(64),
            base_sha: &base,
            base_formula: &fixture.base_formula,
            proposed_formula: &fixture.proposed_formula,
        },
    )
    .expect("canonical historical formula proof");
    let proof_digest = sha256(&canonical_json_bytes(&historical).expect("proof bytes"));
    let candidate = completed_candidate(fixture, &base, &head, &merge, &branch, &proof_digest);
    let authority = authority(&candidate);
    let refs = HistoryRefs {
        base: base.clone(),
        head: head.clone(),
        merge: merge.clone(),
        branch: branch.clone(),
        proof_digest: proof_digest.clone(),
        tree_base: "e".repeat(40),
        tree_head: "f".repeat(40),
        blob_base: "1".repeat(40),
        blob_head: "2".repeat(40),
        base_formula: tap_test_support::fixture().base_formula,
    };
    let responses = completed_responses(&candidate, &authority, &refs, PR);
    (candidate, authority, head, responses)
}

fn completed_candidate(
    fixture: tap_test_support::TapFixture,
    base: &str,
    head: &str,
    merge: &str,
    branch: &str,
    proof_digest: &str,
) -> MergeCandidate {
    let current_proof = current_proof(&fixture, merge);
    let current_proof_digest = sha256(&canonical_json_bytes(&current_proof).expect("proof bytes"));
    let receipt_digest = fixture.release.receipt_digest.clone();
    let formula_path = fixture.formula_path.clone();
    MergeCandidate {
        policy: fixture.policy,
        spec: fixture.spec,
        sdk_artifact_digest: "9".repeat(64),
        plan: fixture.plan,
        release: fixture.release,
        proof: current_proof,
        promotion: TapPromotionReceipt {
            schema: "velnor.homebrew-tap-promotion.v2".to_owned(),
            repository: "owner/homebrew-tap".to_owned(),
            release_receipt_digest: receipt_digest,
            preparation_proof_digest: current_proof_digest,
            outcome: TapPromotionOutcome::AlreadyPromoted {
                pull_request: PR,
                head_sha: head.to_owned(),
                base_sha: base.to_owned(),
                branch: branch.to_owned(),
                candidate_proof_digest: proof_digest.to_owned(),
                merge_sha: merge.to_owned(),
                merged_at: "2026-10-18T10:20:30Z".to_owned(),
            },
        },
        formula: fixture.proposed_formula,
        formula_path,
    }
}

fn current_proof(
    fixture: &tap_test_support::TapFixture,
    base: &str,
) -> HomebrewTapPreparationProof {
    let formula_sha = sha256(&fixture.proposed_formula);
    HomebrewTapPreparationProof {
        schema: "velnor.homebrew-candidate.v4".to_owned(),
        channel: fixture.release.receipt.channel,
        disposition: TapPreparationDisposition::FormulaAlreadyCurrent,
        base_sha: base.to_owned(),
        formula_path: fixture.formula_path.clone(),
        old_formula_sha256: formula_sha.clone(),
        new_formula_sha256: formula_sha,
        receipt_digest: fixture.release.receipt_digest.clone(),
        plan_digest: sha256(&fixture.plan.canonical_bytes().expect("plan bytes")),
        formula_spec_digest: fixture.spec.digest().expect("spec digest"),
        sdk_artifact_digest: "9".repeat(64),
        tap_config_digest: sha256(&canonical_json_bytes(&fixture.policy).expect("policy bytes")),
        tap_repository: fixture.policy.repository.clone(),
        source_repository: fixture.plan.repository.clone(),
        source_sha: fixture.release.receipt.source.commit_sha.clone(),
        version: fixture.release.receipt.version.value.clone(),
        candidate_formula: "formula.rb".to_owned(),
    }
}

pub(super) fn authority(candidate: &MergeCandidate) -> MergeAuthority {
    MergeAuthority {
        policy: velnor_actions_contract::homebrew_merge_gate::TapMergePolicy {
            repository: candidate.policy.repository.clone(),
            writer_app_id: WRITER_APP,
            completion_app_id: COMPLETION_APP,
            completion_check_name: candidate.policy.review_completion.check_name.clone(),
            expected_reviewer_app_ids: [REVIEWER_APP].into_iter().collect(),
        },
        writer_app_id: WRITER_APP,
        completion_app_id: COMPLETION_APP,
        completion_name: candidate.policy.review_completion.check_name.clone(),
        reviewer_ids: [REVIEWER_APP].into_iter().collect(),
        bot_login: "tap-writer[bot]".to_owned(),
        bot_user_id: 77,
        repo_id: 41,
    }
}

pub(super) struct HistoryRefs {
    pub(super) base: String,
    pub(super) head: String,
    pub(super) merge: String,
    pub(super) branch: String,
    pub(super) proof_digest: String,
    pub(super) tree_base: String,
    pub(super) tree_head: String,
    pub(super) blob_base: String,
    pub(super) blob_head: String,
    pub(super) base_formula: Vec<u8>,
}
