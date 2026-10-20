use super::super::super::helpers::{branch_name, sha256};
use super::super::find_already_promoted;
use crate::tap_prepare::reconstruct_historical_candidate_proof;
use crate::{
    tap_prepare::writer::github::ScriptedResponse,
    tap_prepare::writer::gitops::{expected_pr_body, expected_pr_title, signed_off_message},
    tap_prepare::writer::identity::BotIdentity,
    tap_prepare::writer::{TapGitHubApi, WriterInputs},
    tap_prepare::{HomebrewTapPreparationProof, TapPreparationDisposition},
    tap_test_support,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::path::PathBuf;
use velnor_actions_contract::canonical_json_bytes;

#[test]
fn already_promoted_authenticates_list_detail_and_git_history_without_writes() {
    let (inputs, bot, expected) = already_promoted_case();
    let promoted = find_already_promoted(&inputs, &bot).expect("historical PR authenticates");
    assert_eq!(promoted.pull_request, 21);
    assert_eq!(promoted.head_sha, expected.head_sha);
    assert_eq!(promoted.base_sha, expected.base_sha);
    assert_eq!(promoted.merge_sha, expected.merge_sha);
    assert_eq!(promoted.candidate_proof_digest, expected.proof_digest);
    assert!(
        inputs.api.scripted_is_empty(),
        "no mutation or extra request is allowed"
    );
}

struct ExpectedPromotion {
    head_sha: String,
    base_sha: String,
    merge_sha: String,
    proof_digest: String,
}

fn already_promoted_case() -> (WriterInputs, BotIdentity, ExpectedPromotion) {
    let fixture = tap_test_support::fixture();
    let proof = current_proof(&fixture);
    let original = historical_identity(&fixture);
    let responses = historical_responses(&fixture, &original);
    let api = TapGitHubApi::scripted(fixture.policy.repository.clone(), responses);
    let inputs = WriterInputs {
        root: PathBuf::from("/tmp/unused-writer-test"),
        policy: fixture.policy,
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
    (inputs, original.bot, original.expected)
}

fn current_proof(fixture: &tap_test_support::TapFixture) -> HomebrewTapPreparationProof {
    let formula_sha = sha256(&fixture.proposed_formula);
    HomebrewTapPreparationProof {
        schema: "velnor.homebrew-candidate.v3".to_owned(),
        channel: fixture.release.receipt.channel,
        disposition: TapPreparationDisposition::FormulaAlreadyCurrent,
        base_sha: "a".repeat(40),
        formula_path: fixture.formula_path.clone(),
        old_formula_sha256: formula_sha.clone(),
        new_formula_sha256: formula_sha,
        receipt_digest: fixture.release.receipt_digest.clone(),
        plan_digest: sha256(&canonical_json_bytes(&fixture.plan).expect("plan JSON")),
        tap_config_digest: sha256(&canonical_json_bytes(&fixture.policy).expect("policy JSON")),
        tap_repository: fixture.policy.repository.clone(),
        source_repository: fixture.release.receipt.repository.slug.clone(),
        source_sha: fixture.release.receipt.source.commit_sha.clone(),
        version: fixture.release.receipt.version.value.clone(),
        candidate_formula: "formula.rb".to_owned(),
    }
}

struct HistoricalIdentity {
    bot: BotIdentity,
    base_sha: String,
    head_sha: String,
    merge_sha: String,
    base_tree_sha: String,
    head_tree_sha: String,
    base_blob_sha: String,
    head_blob_sha: String,
    signed_message: String,
    branch: String,
    title: String,
    body: String,
    expected: ExpectedPromotion,
}

fn historical_identity(fixture: &tap_test_support::TapFixture) -> HistoricalIdentity {
    let base_sha = "b".repeat(40);
    let head_sha = "c".repeat(40);
    let merge_sha = "d".repeat(40);
    let original_proof = reconstruct_historical_candidate_proof(
        &fixture.plan.repository,
        &fixture.policy,
        &fixture.plan,
        &fixture.release,
        &base_sha,
        &fixture.base_formula,
        &fixture.proposed_formula,
    )
    .expect("historical proof");
    let proof_digest = sha256(&canonical_json_bytes(&original_proof).expect("proof JSON"));
    let bot = BotIdentity {
        login: "tap-writer[bot]".to_owned(),
        user_id: 77,
    };
    let branch = branch_name(
        &fixture.release.receipt_digest,
        &base_sha,
        fixture.release.receipt.channel,
    )
    .expect("branch");
    let title = expected_pr_title(&fixture.release);
    let body = expected_pr_body(&fixture.release, &proof_digest, &base_sha);
    HistoricalIdentity {
        signed_message: signed_off_message(&original_proof, &bot),
        bot,
        expected: ExpectedPromotion {
            head_sha: head_sha.clone(),
            base_sha: base_sha.clone(),
            merge_sha: merge_sha.clone(),
            proof_digest: proof_digest.clone(),
        },
        base_sha,
        head_sha,
        merge_sha,
        base_tree_sha: "e".repeat(40),
        head_tree_sha: "f".repeat(40),
        base_blob_sha: "1".repeat(40),
        head_blob_sha: "2".repeat(40),
        branch,
        title,
        body,
    }
}

fn historical_responses(
    fixture: &tap_test_support::TapFixture,
    original: &HistoricalIdentity,
) -> Vec<ScriptedResponse> {
    let pull = pull_json(
        &fixture.policy.repository,
        &original.branch,
        &original.head_sha,
        &original.base_sha,
        &original.title,
        &original.body,
        &original.bot,
    );
    let mut detail = pull.clone();
    detail["merged"] = serde_json::json!(true);
    detail["merged_at"] = serde_json::json!("2026-10-18T10:20:30Z");
    detail["merge_commit_sha"] = serde_json::json!(original.merge_sha);
    let listing = format!(
        "repos/{}/pulls?state=all&per_page=100&page=1",
        fixture.policy.repository
    );
    let detail_route = format!("repos/{}/pulls/21", fixture.policy.repository);
    let (head_route, base_route, head_tree_route, base_tree_route, merge_route) =
        git_routes(fixture, original);
    let unrelated = unrelated_marker_pull(&pull, &fixture.release.receipt_digest);
    vec![
        json_response("GET", listing, serde_json::json!([pull, unrelated])),
        json_response("GET", detail_route, detail),
        json_response(
            "GET",
            head_route,
            commit_json(
                &original.head_sha,
                &original.head_tree_sha,
                &original.base_sha,
                &original.signed_message,
            ),
        ),
        json_response(
            "GET",
            base_route,
            commit_json(&original.base_sha, &original.base_tree_sha, "", "base"),
        ),
        json_response(
            "GET",
            head_tree_route,
            tree_json(
                &original.head_tree_sha,
                &fixture.formula_path,
                &original.head_blob_sha,
            ),
        ),
        json_response(
            "GET",
            base_tree_route,
            tree_json(
                &original.base_tree_sha,
                &fixture.formula_path,
                &original.base_blob_sha,
            ),
        ),
        blob_response(
            &fixture.policy.repository,
            &original.base_blob_sha,
            &fixture.base_formula,
        ),
        blob_response(
            &fixture.policy.repository,
            &original.head_blob_sha,
            &fixture.proposed_formula,
        ),
        json_response(
            "GET",
            merge_route,
            commit_json(
                &original.merge_sha,
                &original.head_tree_sha,
                &original.base_sha,
                "squash",
            ),
        ),
    ]
}

fn unrelated_marker_pull(pull: &serde_json::Value, receipt_digest: &str) -> serde_json::Value {
    let mut unrelated = pull.clone();
    unrelated["number"] = serde_json::json!(22);
    unrelated["head"]["ref"] = serde_json::json!("feature/unrelated");
    unrelated["head"]["label"] = serde_json::json!("owner:feature/unrelated");
    unrelated["body"] = serde_json::json!(format!(
        "Unrelated discussion quoting Receipt digest: `{receipt_digest}`"
    ));
    unrelated
}

fn git_routes(
    fixture: &tap_test_support::TapFixture,
    original: &HistoricalIdentity,
) -> (String, String, String, String, String) {
    let repo = &fixture.policy.repository;
    (
        format!("repos/{repo}/git/commits/{}", original.head_sha),
        format!("repos/{repo}/git/commits/{}", original.base_sha),
        format!(
            "repos/{repo}/git/trees/{}?recursive=1",
            original.head_tree_sha
        ),
        format!(
            "repos/{repo}/git/trees/{}?recursive=1",
            original.base_tree_sha
        ),
        format!("repos/{repo}/git/commits/{}", original.merge_sha),
    )
}

#[test]
fn current_formula_without_historical_pull_is_not_already_promoted() {
    let fixture = tap_test_support::fixture();
    let current_formula = fixture.proposed_formula.clone();
    let proof = current_proof(&fixture);
    let api = TapGitHubApi::scripted(
        fixture.policy.repository.clone(),
        [json_response(
            "GET",
            format!(
                "repos/{}/pulls?state=all&per_page=100&page=1",
                fixture.policy.repository
            ),
            serde_json::json!([]),
        )],
    );
    let inputs = WriterInputs {
        root: PathBuf::from("/tmp/unused-writer-test"),
        policy: fixture.policy,
        plan: fixture.plan,
        release: fixture.release,
        formula_path: fixture.formula_path.clone(),
        proof,
        proposed: current_formula,
        api,
        app_id: 9,
        installation_id: 10,
        app_slug: "tap-writer".to_owned(),
    };
    let bot = BotIdentity {
        login: "tap-writer[bot]".to_owned(),
        user_id: 77,
    };
    assert!(find_already_promoted(&inputs, &bot).is_err());
    assert!(inputs.api.scripted_is_empty(), "no mutation is allowed");
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

fn pull_json(
    repository: &str,
    branch: &str,
    head: &str,
    base: &str,
    title: &str,
    body: &str,
    bot: &BotIdentity,
) -> serde_json::Value {
    let owner = repository.split_once('/').expect("slug").0;
    serde_json::json!({
        "number": 21, "title": title, "body": body, "state": "closed",
        "user": {"id": bot.user_id, "login": bot.login},
        "head": {"sha": head, "label": format!("{owner}:{branch}"), "ref": branch, "repo": {"full_name": repository}},
        "base": {"sha": base, "label": format!("{owner}:main"), "ref": "main", "repo": {"full_name": repository}}
    })
}

fn commit_json(sha: &str, tree: &str, parent: &str, message: &str) -> serde_json::Value {
    let parents = if parent.is_empty() {
        Vec::<serde_json::Value>::new()
    } else {
        vec![serde_json::json!({"sha": parent, "type": "commit"})]
    };
    serde_json::json!({"sha": sha, "tree": {"sha": tree, "type": "tree"}, "parents": parents, "message": message})
}

fn tree_json(tree_sha: &str, formula: &str, formula_blob: &str) -> serde_json::Value {
    serde_json::json!({
        "sha": tree_sha, "truncated": false,
        "tree": [
            {"path": formula, "mode": "100644", "type": "blob", "sha": formula_blob},
            {"path": "README.md", "mode": "100644", "type": "blob", "sha": "3".repeat(40)}
        ]
    })
}

fn blob_response(repository: &str, sha: &str, bytes: &[u8]) -> ScriptedResponse {
    json_response(
        "GET",
        format!("repos/{repository}/git/blobs/{sha}"),
        serde_json::json!({
            "sha": sha, "encoding": "base64", "content": STANDARD.encode(bytes)
        }),
    )
}
