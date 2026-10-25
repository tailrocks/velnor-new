use super::*;
use std::collections::BTreeSet;

use velnor_actions_contract::{
    canonical_json_bytes, digest_b3,
    homebrew_merge_gate::{
        TapFeedbackSnapshot, TapMergeObservation, TapMergePolicy, TapRequiredCheck,
        TapReviewCompletion, TapServerMergeProtection, validate_tap_merge_gate,
    },
    homebrew_review_completion::{
        TAP_REVIEW_COMPLETION_SCHEMA, TapCompletedReviewer, TapCompletionCheckMetadata,
        TapReviewCompletionPayload, TapReviewVerdict, parse_tap_review_completion,
    },
    package_release::{
        PACKAGE_RELEASE_SCHEMA_V2, PackageReleaseChannel, PackageReleaseEvent,
        PackageReleaseEvidence, PackageReleaseReceipt, PackageReleaseRepository,
        PackageReleaseSbomFormat, PackageReleaseSource, PackageReleaseVersion,
        PackageReleaseWorkflow,
    },
};

#[test]
fn incomplete_observation_returns_pending_once_without_refresh_or_merge() {
    let policy = TapMergePolicy {
        repository: "owner/tap".to_owned(),
        writer_app_id: 11,
        completion_app_id: 12,
        completion_check_name: "Review".to_owned(),
        expected_reviewer_app_ids: BTreeSet::from([13]),
    };
    let mut observations = 0;
    let outcome = attempt_once(
        &policy,
        || {
            observations += 1;
            Ok(None)
        },
        || panic!("pending must not run final source admission"),
        |_| panic!("pending must not issue a merge"),
    )
    .expect("bounded pending observation");
    assert_eq!(outcome, TapMergeOutcome::Pending);
    assert_eq!(observations, 1);
}

#[test]
fn incomplete_feedback_waits_only_until_completion_and_terminal_findings_fail() {
    let head = "a".repeat(40);
    let mut observed = TapMergeObservation {
        pull_request_number: 5,
        validated_base_sha: "b".repeat(40),
        current_base_sha: "b".repeat(40),
        validated_head_sha: head.clone(),
        current_head_sha: head.clone(),
        expected_open_pull_request: true,
        server_protection: TapServerMergeProtection::PullRequestProtected,
        required_checks_complete: true,
        checks: vec![
            TapRequiredCheck {
                name: "DCO".to_owned(),
                app_id: 10,
                required_app_id: Some(10),
                head_sha: head.clone(),
                completed_successfully: true,
            },
            TapRequiredCheck {
                name: "Review".to_owned(),
                app_id: 12,
                required_app_id: Some(12),
                head_sha: head.clone(),
                completed_successfully: true,
            },
        ],
        feedback: TapFeedbackSnapshot {
            repository: "owner/tap".to_owned(),
            pull_request_number: 5,
            head_sha: head.clone(),
            records_digest: format!("b3-{}", "c".repeat(64)),
            unresolved_threads: 0,
            unaddressed_findings: 0,
            outstanding_change_requests: 0,
        },
        completion: None,
        historical_feedback_complete: true,
        historical_feedback: Vec::new(),
    };
    let policy = TapMergePolicy {
        repository: "owner/tap".to_owned(),
        writer_app_id: 11,
        completion_app_id: 12,
        completion_check_name: "Review".to_owned(),
        expected_reviewer_app_ids: BTreeSet::from([13]),
    };
    assert!(!gate_ready_or_pending(&policy, &observed).expect("completion pending"));
    observed.feedback.unresolved_threads = 1;
    assert!(!gate_ready_or_pending(&policy, &observed).expect("review disposition pending"));
    observed.completion = Some(TapReviewCompletion {
        repository: "owner/tap".to_owned(),
        pull_request_number: 5,
        check_name: "Review".to_owned(),
        app_id: 12,
        head_sha: head,
        completed_successfully: true,
        feedback_digest: observed
            .feedback
            .canonical_digest()
            .expect("feedback digest"),
        completed_reviewer_app_ids: BTreeSet::from([13]),
    });
    assert!(gate_ready_or_pending(&policy, &observed).is_err());
}

#[test]
fn canonical_receipt_to_completion_to_merge_contract_uses_real_digest_formats() {
    let (receipt_digest, policy, observation) = ready_gate_fixture();
    validate_tap_merge_gate(&policy, &observation).expect("exact-head admission");
    let mut blocked = observation;
    blocked.feedback.unresolved_threads = 1;
    assert!(validate_tap_merge_gate(&policy, &blocked).is_err());
    assert!(receipt_digest.starts_with("b3-"));
}

fn ready_gate_fixture() -> (String, TapMergePolicy, TapMergeObservation) {
    let (receipt_digest, base_sha, promotion) = promotion_fixture();
    let feedback = TapFeedbackSnapshot {
        repository: "owner/tap".to_owned(),
        pull_request_number: pull_number(&promotion),
        head_sha: pull_head(&promotion).to_owned(),
        records_digest: digest_b3(b"complete feedback records"),
        unresolved_threads: 0,
        unaddressed_findings: 0,
        outstanding_change_requests: 0,
    };
    let completion = completion_for(&promotion, &feedback);
    let policy = TapMergePolicy {
        repository: "owner/tap".to_owned(),
        writer_app_id: 17,
        completion_app_id: 19,
        completion_check_name: "Review completion".to_owned(),
        expected_reviewer_app_ids: BTreeSet::from([23]),
    };
    let observation = TapMergeObservation {
        pull_request_number: pull_number(&promotion),
        validated_base_sha: base_sha.clone(),
        current_base_sha: base_sha,
        validated_head_sha: pull_head(&promotion).to_owned(),
        current_head_sha: pull_head(&promotion).to_owned(),
        expected_open_pull_request: true,
        server_protection: TapServerMergeProtection::PullRequestProtected,
        required_checks_complete: true,
        checks: vec![
            TapRequiredCheck {
                name: "DCO".to_owned(),
                app_id: 29,
                required_app_id: Some(29),
                head_sha: pull_head(&promotion).to_owned(),
                completed_successfully: true,
            },
            TapRequiredCheck {
                name: "Review completion".to_owned(),
                app_id: 19,
                required_app_id: Some(19),
                head_sha: pull_head(&promotion).to_owned(),
                completed_successfully: true,
            },
        ],
        feedback: feedback.clone(),
        completion: Some(completion),
        historical_feedback_complete: true,
        historical_feedback: Vec::new(),
    };
    (receipt_digest, policy, observation)
}

#[test]
fn shared_one_payload_fixture_has_a_valid_canonical_formula_update() {
    let fixture = crate::homebrew::test_support::fixture();
    fixture.plan.validate().expect("static plan");
    fixture
        .release
        .validate(&fixture.plan.repository, &fixture.plan)
        .expect("release matches plan");
    assert_ne!(fixture.base_formula, fixture.proposed_formula);
}

fn promotion_fixture() -> (String, String, crate::tap_prepare::TapPromotionReceipt) {
    let source_sha = "a".repeat(40);
    let receipt = preview_receipt(&source_sha);
    let receipt_digest = receipt
        .canonical_digest()
        .expect("canonical receipt digest");
    let base_sha = "b".repeat(40);
    let branch = candidate::expected_branch(&receipt_digest, &base_sha, receipt.channel)
        .expect("deterministic branch");
    let proof = crate::tap_prepare::HomebrewTapPreparationProof {
        schema: "velnor.homebrew-candidate.v4".to_owned(),
        channel: receipt.channel,
        disposition: crate::tap_prepare::TapPreparationDisposition::Changed,
        base_sha: base_sha.clone(),
        formula_path: "Formula/example.rb".to_owned(),
        old_formula_sha256: "c".repeat(64),
        new_formula_sha256: "d".repeat(64),
        receipt_digest: receipt_digest.clone(),
        plan_digest: "e".repeat(64),
        formula_spec_digest: Some(format!("b3-{}", "1".repeat(64))),
        sdk_artifact_digest: "2".repeat(64),
        tap_config_digest: "f".repeat(64),
        tap_repository: "owner/tap".to_owned(),
        source_repository: "owner/source".to_owned(),
        source_sha,
        version: receipt.version.value.clone(),
        candidate_formula: "formula.rb".to_owned(),
    };
    let proof_digest = sha256(&canonical_json_bytes(&proof).expect("proof bytes"));
    let promotion = crate::tap_prepare::TapPromotionReceipt {
        schema: "velnor.homebrew-tap-pr.v2".to_owned(),
        repository: "owner/tap".to_owned(),
        release_receipt_digest: receipt_digest.clone(),
        preparation_proof_digest: proof_digest.clone(),
        outcome: crate::tap_prepare::TapPromotionOutcome::PullRequest {
            pull_request: 4,
            head_sha: "c".repeat(40),
            base_sha: base_sha.clone(),
            branch,
            candidate_proof_digest: proof_digest,
        },
    };
    assert_eq!(pull_base(&promotion), proof.base_sha);
    (receipt_digest, base_sha, promotion)
}

fn preview_receipt(source_sha: &str) -> PackageReleaseReceipt {
    PackageReleaseReceipt {
        schema: PACKAGE_RELEASE_SCHEMA_V2.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: PackageReleaseRepository {
            slug: "owner/source".to_owned(),
            id: 31,
        },
        source: PackageReleaseSource {
            commit_sha: source_sha.to_owned(),
            source_ref: "refs/heads/main".to_owned(),
            default_branch: "main".to_owned(),
            event: PackageReleaseEvent::WorkflowRun,
            ci_run_id: 1,
            ci_run_attempt: 1,
        },
        workflow: PackageReleaseWorkflow {
            path: ".github/workflows/binary-preview.yml".to_owned(),
            workflow_ref: "refs/heads/main".to_owned(),
            authority_sha: source_sha.to_owned(),
            run_id: 2,
            run_attempt: 1,
            producer_run_id: 2,
            producer_run_attempt: 1,
            signer_job_id: "preview-signer".to_owned(),
            signer_workflow: ".github/workflows/binary-preview.yml".to_owned(),
        },
        version: PackageReleaseVersion {
            base: "1.2.3".to_owned(),
            value: "1.2.3-preview.7+abcdef0".to_owned(),
            commit_count: Some(7),
            source_sha7: Some("abcdef0".to_owned()),
        },
        tag: "v1.2.3-preview.7+abcdef0".to_owned(),
        products: Vec::new(),
        companions: Vec::new(),
        assets: Vec::new(),
        evidence: PackageReleaseEvidence {
            profile: "signed-sbom-v1".to_owned(),
            sbom_format: PackageReleaseSbomFormat::CycloneDxJson,
            signer_issuer: "https://token.actions.githubusercontent.com".to_owned(),
            signer_workflow_ref:
                "owner/source/.github/workflows/binary-preview.yml@refs/heads/main".to_owned(),
            attestation_predicate: "https://slsa.dev/provenance/v1".to_owned(),
            attested_subject_names: Vec::new(),
        },
    }
}

fn completion_for(
    promotion: &crate::tap_prepare::TapPromotionReceipt,
    feedback: &TapFeedbackSnapshot,
) -> TapReviewCompletion {
    let feedback_digest = feedback.canonical_digest().expect("feedback digest");
    let completion_payload = TapReviewCompletionPayload {
        schema: TAP_REVIEW_COMPLETION_SCHEMA.to_owned(),
        repository: "owner/tap".to_owned(),
        pull_request_number: pull_number(promotion),
        head_sha: pull_head(promotion).to_owned(),
        verdict: TapReviewVerdict::Complete,
        feedback_digest,
        has_unaddressed_findings: false,
        reviewers: vec![TapCompletedReviewer {
            app_id: 23,
            head_sha: pull_head(promotion).to_owned(),
            evidence_digest: digest_b3(b"review evidence"),
            complete: true,
            has_unaddressed_findings: false,
        }],
    };
    let completion_bytes = canonical_json_bytes(&completion_payload).expect("completion bytes");
    parse_tap_review_completion(
        &TapCompletionCheckMetadata {
            name: "Review completion".to_owned(),
            app_id: 19,
            head_sha: pull_head(promotion).to_owned(),
            completed_successfully: true,
        },
        &completion_bytes,
    )
    .expect("trusted completion parse")
}

fn pull_number(promotion: &crate::tap_prepare::TapPromotionReceipt) -> u64 {
    match &promotion.outcome {
        crate::tap_prepare::TapPromotionOutcome::PullRequest { pull_request, .. }
        | crate::tap_prepare::TapPromotionOutcome::AlreadyPromoted { pull_request, .. } => {
            *pull_request
        }
        crate::tap_prepare::TapPromotionOutcome::SkippedStaleSource { .. } => {
            panic!("stale source has no PR")
        }
    }
}

fn pull_head(promotion: &crate::tap_prepare::TapPromotionReceipt) -> &str {
    match &promotion.outcome {
        crate::tap_prepare::TapPromotionOutcome::PullRequest { head_sha, .. }
        | crate::tap_prepare::TapPromotionOutcome::AlreadyPromoted { head_sha, .. } => head_sha,
        crate::tap_prepare::TapPromotionOutcome::SkippedStaleSource { .. } => {
            panic!("stale source has no PR")
        }
    }
}

fn pull_base(promotion: &crate::tap_prepare::TapPromotionReceipt) -> &str {
    match &promotion.outcome {
        crate::tap_prepare::TapPromotionOutcome::PullRequest { base_sha, .. }
        | crate::tap_prepare::TapPromotionOutcome::AlreadyPromoted { base_sha, .. } => base_sha,
        crate::tap_prepare::TapPromotionOutcome::SkippedStaleSource { .. } => {
            panic!("stale source has no PR")
        }
    }
}

fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0f)]));
            out
        })
}

#[path = "tap_merge_attempt_tests.rs"]
mod attempts;
