use super::github::ScriptedResponse;
use super::gitops::{
    BranchRequest, CurrentSourceWrite, PullRequestRequest, create_or_reconcile_branch,
    create_or_reconcile_pr, expected_pr_body, expected_pr_title,
};
use super::helpers::{branch_name, sha256};
use super::{BotIdentity, TapGitHubApi};
use crate::{
    homebrew::test_support as tap_test_support,
    tap_prepare::{TapPreparationDisposition, reconstruct_historical_candidate_proof},
};
use velnor_actions_contract::canonical_json_bytes;

#[test]
fn accepted_pr_create_with_lost_response_reconciles_without_a_second_post() {
    let fixture = tap_test_support::fixture();
    let base_sha = "b".repeat(40);
    let head_sha = "c".repeat(40);
    let proof =
        reconstruct_historical_candidate_proof(crate::tap_prepare::HistoricalCandidateInput {
            source_repository: &fixture.plan.repository,
            tap: &fixture.policy,
            plan: &fixture.plan,
            release: &fixture.release,
            spec: Some(&fixture.spec),
            sdk_artifact_digest: &"9".repeat(64),
            base_sha: &base_sha,
            base_formula: &fixture.base_formula,
            proposed_formula: &fixture.proposed_formula,
        })
        .expect("validated candidate proof");
    let proof_digest = sha256(&canonical_json_bytes(&proof).expect("proof JSON"));
    assert_eq!(proof.disposition, TapPreparationDisposition::Changed);
    let branch = branch_name(
        &fixture.release.receipt_digest,
        &base_sha,
        fixture.release.receipt.channel,
    )
    .expect("branch");
    let bot = BotIdentity {
        login: "tap-writer[bot]".to_owned(),
        user_id: 77,
    };
    let query = format!(
        "repos/{}/pulls?head=owner:{}&base=main&state=all&per_page=100",
        fixture.policy.repository,
        super::github::encode_segment(&branch),
    );
    let pr = expected_open_pr(
        &fixture.policy.repository,
        &branch,
        &head_sha,
        &base_sha,
        &proof_digest,
        &fixture.release,
        &bot,
    );
    let api = TapGitHubApi::scripted(
        fixture.policy.repository.clone(),
        [
            json_response("GET", query.clone(), serde_json::json!([])),
            ScriptedResponse {
                method: "POST",
                route: format!("repos/{}/pulls", fixture.policy.repository),
                body: b"accepted but response was lost".to_vec(),
            },
            json_response("GET", query, serde_json::json!([pr])),
        ],
    );
    let mut verify_current = || Ok(true);
    let mut request = PullRequestRequest {
        policy: &fixture.policy,
        branch: &branch,
        head_sha: &head_sha,
        base_sha: &base_sha,
        proof_digest: &proof_digest,
        release: &fixture.release,
        bot: &bot,
        verify_current: &mut verify_current,
    };

    assert!(create_or_reconcile_pr(&api, &mut request).is_err());
    assert_eq!(
        create_or_reconcile_pr(&api, &mut request).expect("retry adopts exact PR"),
        CurrentSourceWrite::Completed(21)
    );
    assert!(
        api.scripted_is_empty(),
        "retry must not issue a second POST"
    );
}

#[test]
fn source_advance_after_pr_creation_returns_typed_skip_without_another_write() {
    let fixture = tap_test_support::fixture();
    let base_sha = "b".repeat(40);
    let head_sha = "c".repeat(40);
    let proof =
        reconstruct_historical_candidate_proof(crate::tap_prepare::HistoricalCandidateInput {
            source_repository: &fixture.plan.repository,
            tap: &fixture.policy,
            plan: &fixture.plan,
            release: &fixture.release,
            spec: Some(&fixture.spec),
            sdk_artifact_digest: &"9".repeat(64),
            base_sha: &base_sha,
            base_formula: &fixture.base_formula,
            proposed_formula: &fixture.proposed_formula,
        })
        .expect("validated candidate proof");
    let proof_digest = sha256(&canonical_json_bytes(&proof).expect("proof JSON"));
    let branch = branch_name(
        &fixture.release.receipt_digest,
        &base_sha,
        fixture.release.receipt.channel,
    )
    .expect("branch");
    let bot = BotIdentity {
        login: "tap-writer[bot]".to_owned(),
        user_id: 77,
    };
    let query = format!(
        "repos/{}/pulls?head=owner:{}&base=main&state=all&per_page=100",
        fixture.policy.repository,
        super::github::encode_segment(&branch),
    );
    let pr = expected_open_pr(
        &fixture.policy.repository,
        &branch,
        &head_sha,
        &base_sha,
        &proof_digest,
        &fixture.release,
        &bot,
    );
    let api = TapGitHubApi::scripted(
        fixture.policy.repository.clone(),
        [
            json_response("GET", query, serde_json::json!([])),
            json_response(
                "POST",
                format!("repos/{}/pulls", fixture.policy.repository),
                pr,
            ),
        ],
    );
    let mut checks = [true, false].into_iter();
    let mut verify_current = || Ok(checks.next().expect("source check"));
    let mut request = PullRequestRequest {
        policy: &fixture.policy,
        branch: &branch,
        head_sha: &head_sha,
        base_sha: &base_sha,
        proof_digest: &proof_digest,
        release: &fixture.release,
        bot: &bot,
        verify_current: &mut verify_current,
    };

    assert!(matches!(
        create_or_reconcile_pr(&api, &mut request).expect("stale source clean skip"),
        CurrentSourceWrite::SkippedStaleSource
    ));
    assert!(api.scripted_is_empty());
}

#[test]
fn source_advance_after_blob_write_returns_typed_skip_before_next_mutation() {
    let fixture = tap_test_support::fixture();
    let base_sha = "b".repeat(40);
    let proof =
        reconstruct_historical_candidate_proof(crate::tap_prepare::HistoricalCandidateInput {
            source_repository: &fixture.plan.repository,
            tap: &fixture.policy,
            plan: &fixture.plan,
            release: &fixture.release,
            spec: Some(&fixture.spec),
            sdk_artifact_digest: &"9".repeat(64),
            base_sha: &base_sha,
            base_formula: &fixture.base_formula,
            proposed_formula: &fixture.proposed_formula,
        })
        .expect("validated candidate proof");
    let branch = branch_name(
        &fixture.release.receipt_digest,
        &base_sha,
        fixture.release.receipt.channel,
    )
    .expect("branch");
    let bot = BotIdentity {
        login: "tap-writer[bot]".to_owned(),
        user_id: 77,
    };
    let api = TapGitHubApi::scripted(
        fixture.policy.repository.clone(),
        [json_response(
            "POST",
            format!("repos/{}/git/blobs", fixture.policy.repository),
            serde_json::json!({"sha":"e".repeat(40)}),
        )],
    );
    let base_tree_sha = "d".repeat(40);
    let mut checks = [true, false].into_iter();
    let mut verify_current = || Ok(checks.next().expect("source check"));
    let mut request = BranchRequest {
        proof: &proof,
        formula: &fixture.proposed_formula,
        base_sha: &base_sha,
        base_tree_sha: &base_tree_sha,
        branch: &branch,
        bot: &bot,
        verify_current: &mut verify_current,
    };

    assert!(matches!(
        create_or_reconcile_branch(&api, &mut request).expect("stale source clean skip"),
        CurrentSourceWrite::SkippedStaleSource
    ));
    assert!(api.scripted_is_empty());
}

#[test]
fn source_ci_or_authentication_errors_are_not_converted_to_skip() {
    let fixture = tap_test_support::fixture();
    let release = &fixture.release;
    let branch = "velnor/preview/exact";
    let head_sha = "c".repeat(40);
    let base_sha = "b".repeat(40);
    let bot = BotIdentity {
        login: "tap-writer[bot]".to_owned(),
        user_id: 77,
    };
    let api = TapGitHubApi::scripted(
        fixture.policy.repository.clone(),
        [json_response(
            "GET",
            format!(
                "repos/{}/pulls?head=owner:{}&base=main&state=all&per_page=100",
                fixture.policy.repository,
                super::github::encode_segment(branch),
            ),
            serde_json::json!([]),
        )],
    );
    let proof_digest = sha256(b"proof");
    let mut verify_current = || Err(crate::internal::internal("latest_source_ci_failed"));
    let mut request = PullRequestRequest {
        policy: &fixture.policy,
        branch,
        head_sha: &head_sha,
        base_sha: &base_sha,
        proof_digest: &proof_digest,
        release,
        bot: &bot,
        verify_current: &mut verify_current,
    };

    let error = create_or_reconcile_pr(&api, &mut request).expect_err("CI failure propagates");
    assert!(error.to_string().contains("latest_source_ci_failed"));
    assert!(api.scripted_is_empty());
}

fn expected_open_pr(
    repository: &str,
    branch: &str,
    head_sha: &str,
    base_sha: &str,
    proof_digest: &str,
    release: &crate::tap_prepare::writer::VerifiedPackageRelease,
    bot: &BotIdentity,
) -> serde_json::Value {
    let owner = repository.split_once('/').expect("repository owner").0;
    serde_json::json!({
        "number": 21,
        "title": expected_pr_title(release),
        "body": expected_pr_body(release, proof_digest, base_sha),
        "state": "open",
        "merged": false,
        "user": {"id": bot.user_id, "login": bot.login},
        "head": {"sha": head_sha, "label": format!("{owner}:{branch}"), "ref": branch, "repo": {"full_name": repository}},
        "base": {"sha": base_sha, "label": format!("{owner}:main"), "ref": "main", "repo": {"full_name": repository}}
    })
}

fn json_response<T: serde::Serialize>(
    method: &'static str,
    route: String,
    value: T,
) -> ScriptedResponse {
    ScriptedResponse {
        method,
        route,
        body: serde_json::to_vec(&value).expect("JSON response"),
    }
}
