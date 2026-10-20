use super::github::ScriptedResponse;
use super::gitops::{expected_pr_body, expected_pr_title};
use super::helpers::{branch_name, sha256};
use super::{BotIdentity, PullRequestRequest, TapGitHubApi, create_or_reconcile_pr};
use crate::{
    tap_prepare::{TapPreparationDisposition, reconstruct_historical_candidate_proof},
    tap_test_support,
};
use velnor_actions_contract::canonical_json_bytes;

#[test]
fn accepted_pr_create_with_lost_response_reconciles_without_a_second_post() {
    let fixture = tap_test_support::fixture();
    let base_sha = "b".repeat(40);
    let head_sha = "c".repeat(40);
    let proof = reconstruct_historical_candidate_proof(
        &fixture.plan.repository,
        &fixture.policy,
        &fixture.plan,
        &fixture.release,
        &base_sha,
        &fixture.base_formula,
        &fixture.proposed_formula,
    )
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
    let mut verify_current = || Ok(());
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
        21
    );
    assert!(
        api.scripted_is_empty(),
        "retry must not issue a second POST"
    );
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
