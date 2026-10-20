use super::super::{candidate::MergeCandidate, observation::MergeAuthority};
use super::response_helpers::{content, content_route, opinions, pull, put, sha256, tree};
use crate::tap_test_support;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use velnor_actions_contract::canonical_json_bytes;

pub(super) fn add_closed_prior_pr(
    responses: &mut BTreeMap<String, Value>,
    candidate: &MergeCandidate,
    authority: &MergeAuthority,
) {
    let original_base = "9".repeat(40);
    let head = "8".repeat(40);
    let advanced_base = "d".repeat(40);
    let formula = tap_test_support::fixture();
    let proof = crate::tap_prepare::reconstruct_historical_candidate_proof(
        &candidate.plan.repository,
        &candidate.policy,
        &candidate.plan,
        &candidate.release,
        &original_base,
        &formula.base_formula,
        &candidate.formula,
    )
    .expect("old formula candidate proof");
    let proof_digest = sha256(&canonical_json_bytes(&proof).expect("old proof bytes"));
    let branch = super::super::candidate::expected_branch(
        &candidate.release.receipt_digest,
        &original_base,
        candidate.release.receipt.channel,
    )
    .expect("old deterministic branch");
    let summary = pull(
        candidate,
        authority,
        20,
        &branch,
        &head,
        &original_base,
        &proof_digest,
    );
    let mut detail = summary.clone();
    detail["base"]["sha"] = json!(advanced_base);
    detail["merged"] = json!(false);
    replace_summary(responses, candidate, &summary);
    responses.insert(
        format!("repos/{}/pulls/20", candidate.policy.repository),
        detail,
    );
    add_prior_git_responses(
        responses,
        candidate,
        authority,
        &formula,
        &original_base,
        &head,
    );
    responses.insert(
        "graphql:threads:20".to_owned(),
        json!({"data": {"repository": {"pullRequest": {"reviewThreads": {
            "nodes": [{"id": "PRRT_prior", "isResolved": false, "isOutdated": true}],
            "pageInfo": {"hasNextPage": false}
        }}}}}),
    );
}

fn replace_summary(
    responses: &mut BTreeMap<String, Value>,
    candidate: &MergeCandidate,
    prior: &Value,
) {
    let route = format!(
        "repos/{}/pulls?state=all&per_page=100&page=1",
        candidate.policy.repository
    );
    let selected = responses[&route][0].clone();
    responses.insert(route, json!([prior.clone(), selected]));
}

fn add_prior_git_responses(
    responses: &mut BTreeMap<String, Value>,
    candidate: &MergeCandidate,
    authority: &MergeAuthority,
    fixture: &tap_test_support::TapFixture,
    base: &str,
    head: &str,
) {
    let tree_base = "7".repeat(40);
    let tree_head = "6".repeat(40);
    let blob_base = "5".repeat(40);
    let blob_head = "4".repeat(40);
    let repo = &candidate.policy.repository;
    put(
        responses,
        format!("repos/{repo}/pulls/20/commits?per_page=100&page=1"),
        json!([{"sha": head}]),
    );
    let formula = &candidate.formula_path;
    let message = format!(
        "Update {} formula to {}\n\nSource: {}@{}\nRelease receipt: {}\n\nSigned-off-by: {} <{}+{}@users.noreply.github.com>",
        match candidate.release.receipt.channel {
            velnor_actions_contract::package_release::PackageReleaseChannel::Preview => "preview",
            velnor_actions_contract::package_release::PackageReleaseChannel::Stable => "stable",
        },
        candidate.release.receipt.version.value,
        candidate.plan.repository,
        candidate.release.receipt.source.commit_sha,
        candidate.release.receipt_digest,
        authority.bot_login,
        authority.bot_user_id,
        authority.bot_login,
    );
    put(
        responses,
        format!("repos/{repo}/git/commits/{head}"),
        json!({"sha": head, "message": message, "tree": {"sha": tree_head}, "parents": [{"sha": base}]}),
    );
    put(
        responses,
        format!("repos/{repo}/git/commits/{base}"),
        json!({"sha": base, "tree": {"sha": tree_base}, "parents": []}),
    );
    put(
        responses,
        format!("repos/{repo}/git/trees/{tree_base}?recursive=1"),
        tree(&tree_base, formula, &blob_base),
    );
    put(
        responses,
        format!("repos/{repo}/git/trees/{tree_head}?recursive=1"),
        tree(&tree_head, formula, &blob_head),
    );
    put(
        responses,
        content_route(repo, formula, base),
        content(&fixture.base_formula),
    );
    put(
        responses,
        content_route(repo, formula, head),
        content(&candidate.formula),
    );
    put(
        responses,
        format!("repos/{repo}/commits/{head}/comments?per_page=100&page=1"),
        json!([]),
    );
    put(
        responses,
        format!("repos/{repo}/commits/{head}/check-runs?per_page=100&page=1"),
        json!({"check_runs": []}),
    );
    for route in [
        format!("repos/{repo}/pulls/20/reviews?per_page=100&page=1"),
        format!("repos/{repo}/pulls/20/comments?per_page=100&page=1"),
        format!("repos/{repo}/issues/20/comments?per_page=100&page=1"),
    ] {
        put(responses, route, json!([]));
    }
    put(responses, "graphql:opinions:20".to_owned(), opinions());
}
