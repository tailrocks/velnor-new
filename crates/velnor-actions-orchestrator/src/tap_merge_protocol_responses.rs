use super::super::{candidate::MergeCandidate, observation::MergeAuthority};
use super::case::HistoryRefs;
use super::response_helpers::{checks, content, content_route, opinions, pull, put, threads, tree};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const DCO_APP: u64 = 29;

pub(super) fn completed_responses(
    candidate: &MergeCandidate,
    authority: &MergeAuthority,
    refs: &HistoryRefs,
    number: u64,
) -> BTreeMap<String, Value> {
    let mut map = history_responses(candidate, authority, refs, number);
    add_feedback_responses(&mut map, &candidate.policy.repository, number, &refs.head);
    add_current_main_responses(&mut map, candidate, refs);
    add_protection_responses(&mut map, candidate, authority);
    map
}

fn history_responses(
    candidate: &MergeCandidate,
    authority: &MergeAuthority,
    refs: &HistoryRefs,
    number: u64,
) -> BTreeMap<String, Value> {
    let mut map = history_identity_responses(candidate, authority, refs, number);
    map.extend(history_git_responses(candidate, authority, refs, number));
    map
}

fn history_identity_responses(
    candidate: &MergeCandidate,
    authority: &MergeAuthority,
    refs: &HistoryRefs,
    number: u64,
) -> BTreeMap<String, Value> {
    let mut map = BTreeMap::new();
    let repository = &candidate.policy.repository;
    let HistoryRefs {
        base,
        head,
        merge,
        branch,
        proof_digest,
        tree_head,
        ..
    } = refs;
    let commit_message = format!(
        "Update preview formula to {}\n\nSource: {}@{}\nRelease receipt: {}\n\nSigned-off-by: {} <{}+{}@users.noreply.github.com>",
        candidate.release.receipt.version.value,
        candidate.plan.repository,
        candidate.release.receipt.source.commit_sha,
        candidate.release.receipt_digest,
        authority.bot_login,
        authority.bot_user_id,
        authority.bot_login,
    );
    let summary = pull(
        candidate,
        authority,
        number,
        branch,
        head,
        base,
        proof_digest,
    );
    let mut detail = summary.clone();
    detail["merged"] = json!(true);
    detail["merged_at"] = json!("2026-10-18T10:20:30Z");
    detail["merge_commit_sha"] = json!(merge);
    put(
        &mut map,
        format!("repos/{repository}/pulls?state=all&per_page=100&page=1"),
        json!([summary]),
    );
    put(
        &mut map,
        format!("repos/{repository}/pulls/{number}"),
        detail,
    );
    put(
        &mut map,
        format!("repos/{repository}/git/commits/{head}"),
        json!({"sha": head, "message": commit_message, "tree": {"sha": tree_head}, "parents": [{"sha": base}]}),
    );
    map
}

fn history_git_responses(
    candidate: &MergeCandidate,
    authority: &MergeAuthority,
    refs: &HistoryRefs,
    number: u64,
) -> BTreeMap<String, Value> {
    let mut map = BTreeMap::new();
    let repository = &candidate.policy.repository;
    let formula = &candidate.policy.formula;
    let HistoryRefs {
        base,
        head,
        tree_base,
        tree_head,
        blob_base,
        blob_head,
        ..
    } = refs;
    put(
        &mut map,
        format!("repos/{repository}/git/commits/{base}"),
        json!({"sha": base, "tree": {"sha": tree_base}, "parents": []}),
    );
    put(
        &mut map,
        format!("repos/{repository}/git/trees/{tree_base}?recursive=1"),
        tree(tree_base, formula, blob_base),
    );
    put(
        &mut map,
        format!("repos/{repository}/git/trees/{tree_head}?recursive=1"),
        tree(tree_head, formula, blob_head),
    );
    put(
        &mut map,
        content_route(repository, formula, base),
        content(&refs.base_formula),
    );
    put(
        &mut map,
        content_route(repository, formula, head),
        content(&candidate.formula),
    );
    put(
        &mut map,
        format!("repos/{repository}/commits/{head}/check-runs?per_page=100&page=1"),
        checks(candidate, head, authority, number),
    );
    map
}

fn add_feedback_responses(
    map: &mut BTreeMap<String, Value>,
    repository: &str,
    number: u64,
    head: &str,
) {
    put(
        map,
        format!("repos/{repository}/pulls/{number}/commits?per_page=100&page=1"),
        json!([{"sha":head}]),
    );
    put(
        map,
        format!("repos/{repository}/pulls/{number}/reviews?per_page=100&page=1"),
        json!([]),
    );
    put(
        map,
        format!("repos/{repository}/pulls/{number}/comments?per_page=100&page=1"),
        json!([]),
    );
    put(
        map,
        format!("repos/{repository}/issues/{number}/comments?per_page=100&page=1"),
        json!([]),
    );
    put(
        map,
        format!("repos/{repository}/commits/{head}/comments?per_page=100&page=1"),
        json!([]),
    );
    put(map, format!("graphql:threads:{number}"), threads());
    put(map, format!("graphql:opinions:{number}"), opinions());
}

fn add_current_main_responses(
    map: &mut BTreeMap<String, Value>,
    candidate: &MergeCandidate,
    refs: &HistoryRefs,
) {
    let repository = &candidate.policy.repository;
    let formula = &candidate.policy.formula;
    let merge = &refs.merge;
    put(
        map,
        format!("repos/{repository}/commits/main"),
        json!({"sha": merge}),
    );
    put(
        map,
        format!("repos/{repository}/compare/{merge}...{merge}"),
        json!({"status": "identical", "ahead_by": 0, "behind_by": 0}),
    );
    put(
        map,
        content_route(repository, formula, merge),
        content(&candidate.formula),
    );
}

fn add_protection_responses(
    map: &mut BTreeMap<String, Value>,
    candidate: &MergeCandidate,
    authority: &MergeAuthority,
) {
    let repository = &candidate.policy.repository;
    put(
        map,
        format!("repos/{repository}/branches/main/protection"),
        json!({
            "enforce_admins": {"enabled": true},
            "required_pull_request_reviews": {
                "required_approving_review_count": 0,
                "bypass_pull_request_allowances": {"apps": [], "users": [], "teams": []}
            },
            "required_conversation_resolution": {"enabled": true},
            "allow_force_pushes": {"enabled": false},
            "allow_deletions": {"enabled": false},
            "required_status_checks": {"strict": true, "checks": [
                {"context": "DCO", "app_id": DCO_APP},
                {"context": authority.completion_name, "app_id": authority.completion_app_id}
            ]}
        }),
    );
    put(
        map,
        format!("repos/{repository}/rules/branches/main?per_page=100&page=1"),
        json!([
            {"ruleset_id": 5, "ruleset_source_type": "Repository", "ruleset_source": repository}
        ]),
    );
    put(
        map,
        format!("repos/{repository}/rulesets/5?includes_parents=true"),
        json!({
            "id": 5, "source_type": "Repository", "source": repository, "target": "branch",
            "enforcement": "active", "current_user_can_bypass":"never", "bypass_actors": [], "rules": [
                {"type": "pull_request", "parameters": {
                    "required_approving_review_count": 0,
                    "required_review_thread_resolution": true
                }},
                {"type": "required_status_checks", "parameters": {
                    "strict_required_status_checks_policy": true, "required_status_checks": [
                        {"context": "DCO", "integration_id": DCO_APP},
                        {"context": authority.completion_name, "integration_id": authority.completion_app_id}
                    ]
                }},
                {"type": "non_fast_forward"},
                {"type": "deletion"}
            ]
        }),
    );
}
