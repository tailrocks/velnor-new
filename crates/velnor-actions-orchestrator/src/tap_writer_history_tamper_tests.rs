use super::super::gitops::{expected_pr_body, expected_pr_title};
use super::super::helpers::{branch_name, sha256};
use super::find_already_promoted;
use crate::{
    homebrew::test_support as tap_test_support,
    tap_prepare::writer::github::ScriptedResponse,
    tap_prepare::writer::identity::BotIdentity,
    tap_prepare::writer::{TapGitHubApi, WriterInputs},
    tap_prepare::{HomebrewTapPreparationProof, TapPreparationDisposition},
};
use velnor_actions_contract::canonical_json_bytes;

#[test]
fn open_receipt_branch_with_mutated_title_blocks_completed_retry() {
    assert_open_receipt_candidate_rejected(true);
}

#[test]
fn open_receipt_branch_with_mutated_body_marker_blocks_completed_retry() {
    assert_open_receipt_candidate_rejected(false);
}

fn assert_open_receipt_candidate_rejected(mutate_title: bool) {
    let (inputs, bot) = open_receipt_inputs(mutate_title);
    assert!(find_already_promoted(&inputs, &bot).is_err());
    assert!(
        inputs.api.scripted_is_empty(),
        "no historical detail or write is allowed"
    );
}

fn open_receipt_inputs(mutate_title: bool) -> (WriterInputs, BotIdentity) {
    let fixture = tap_test_support::fixture();
    let base_sha = "b".repeat(40);
    let open_base_sha = "c".repeat(40);
    let expected_title = expected_pr_title(&fixture.release);
    let expected_body = expected_pr_body(&fixture.release, &"d".repeat(64), &open_base_sha);
    let valid_closed = pull_summary(
        21,
        "closed",
        &branch_name(
            &fixture.release.receipt_digest,
            &base_sha,
            fixture.release.receipt.channel,
        )
        .expect("closed branch"),
        &expected_title,
        &expected_pr_body(&fixture.release, &"e".repeat(64), &base_sha),
    );
    let open_branch = branch_name(
        &fixture.release.receipt_digest,
        &open_base_sha,
        fixture.release.receipt.channel,
    )
    .expect("open branch");
    let open_title = if mutate_title {
        "Changed title".to_owned()
    } else {
        expected_title
    };
    let open_body = if mutate_title {
        expected_body
    } else {
        "edited body without receipt marker".to_owned()
    };
    let open = pull_summary(22, "open", &open_branch, &open_title, &open_body);
    let route = format!(
        "repos/{}/pulls?state=all&per_page=100&page=1",
        fixture.policy.repository
    );
    let api = scripted_history_api(&fixture, route, valid_closed, open);
    let formula_sha = sha256(&fixture.proposed_formula);
    let proof = HomebrewTapPreparationProof {
        schema: "velnor.homebrew-candidate.v4".to_owned(),
        channel: fixture.release.receipt.channel,
        disposition: TapPreparationDisposition::FormulaAlreadyCurrent,
        base_sha: "f".repeat(40),
        formula_path: fixture.formula_path.clone(),
        old_formula_sha256: formula_sha.clone(),
        new_formula_sha256: formula_sha,
        receipt_digest: fixture.release.receipt_digest.clone(),
        plan_digest: sha256(&canonical_json_bytes(&fixture.plan).expect("plan JSON")),
        formula_spec_digest: Some(fixture.spec.digest().expect("spec digest")),
        sdk_artifact_digest: "9".repeat(64),
        tap_config_digest: sha256(&canonical_json_bytes(&fixture.policy).expect("policy JSON")),
        tap_repository: fixture.policy.repository.clone(),
        source_repository: fixture.plan.repository.clone(),
        source_sha: fixture.release.receipt.source.commit_sha.clone(),
        version: fixture.release.receipt.version.value.clone(),
        candidate_formula: "formula.rb".to_owned(),
    };
    let inputs = WriterInputs {
        root: std::path::PathBuf::from("/tmp/unused-writer-test"),
        policy: fixture.policy,
        spec: Some(fixture.spec),
        sdk_artifact_digest: "9".repeat(64),
        plan: fixture.plan,
        release: fixture.release,
        formula_path: fixture.formula_path.clone(),
        proof,
        proposed: fixture.proposed_formula,
        api,
        app_id: 9,
        installation_id: 10,
        app_slug: "tap-writer".to_owned(),
    };
    let bot = BotIdentity {
        login: "tap-writer[bot]".to_owned(),
        user_id: 77,
    };
    (inputs, bot)
}

fn scripted_history_api(
    fixture: &tap_test_support::TapFixture,
    route: String,
    valid_closed: serde_json::Value,
    open: serde_json::Value,
) -> TapGitHubApi {
    let response = ScriptedResponse {
        method: "GET",
        route,
        body: serde_json::to_vec(&vec![valid_closed, open]).expect("list JSON"),
    };
    TapGitHubApi::scripted(fixture.policy.repository.clone(), [response])
}

fn pull_summary(
    number: u64,
    state: &str,
    branch: &str,
    title: &str,
    body: &str,
) -> serde_json::Value {
    serde_json::json!({
        "number": number,
        "title": title,
        "body": body,
        "state": state,
        "user": {"id": 77, "login": "tap-writer[bot]"},
        "head": {"sha": "a".repeat(40), "label": format!("owner:{branch}"), "ref": branch, "repo": {"full_name": "owner/homebrew-tap"}},
        "base": {"sha": "b".repeat(40), "label": "owner:main", "ref": "main", "repo": {"full_name": "owner/homebrew-tap"}}
    })
}
