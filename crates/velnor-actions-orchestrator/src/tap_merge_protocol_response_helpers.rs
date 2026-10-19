use super::super::{
    candidate::MergeCandidate,
    observation::{MergeAuthority, expected_pr_body_for, expected_pr_title},
};
use base64::Engine as _;
use serde_json::{Value, json};
use std::{collections::BTreeMap, ffi::OsString};
use velnor_actions_contract::{
    canonical_json_bytes, digest_b3,
    homebrew_merge_gate::TapFeedbackSnapshot,
    homebrew_review_completion::{
        TAP_REVIEW_COMPLETION_SCHEMA, TapCompletedReviewer, TapReviewCompletionPayload,
        TapReviewVerdict,
    },
};

const DCO_APP: u64 = 29;
const REVIEWER_APP: u64 = 23;

pub(super) fn pull(
    candidate: &MergeCandidate,
    authority: &MergeAuthority,
    number: u64,
    branch: &str,
    head: &str,
    base: &str,
    proof_digest: &str,
) -> Value {
    json!({"number": number, "state": "closed", "draft": false,
        "title": expected_pr_title(candidate),
        "body": expected_pr_body_for(candidate, base, proof_digest),
        "base": {"ref": "main", "sha": base, "repo": {"id": 41, "full_name": candidate.policy.repository}},
        "head": {"ref": branch, "sha": head, "label": format!("owner:{branch}"), "repo": {"id": 41, "full_name": candidate.policy.repository}},
        "user": {"login": authority.bot_login, "id": authority.bot_user_id}})
}

pub(super) fn checks(
    candidate: &MergeCandidate,
    head: &str,
    authority: &MergeAuthority,
    number: u64,
) -> Value {
    let records = json!({"reviews": [], "opinionated_reviews": [], "review_comments": [], "issue_comments": [], "commit_comments": [], "threads": []});
    let records_digest = digest_b3(
        &velnor_actions_contract::canonical_json_bytes(&records).expect("feedback records"),
    );
    let feedback_digest = TapFeedbackSnapshot {
        repository: candidate.policy.repository.clone(),
        pull_request_number: number,
        head_sha: head.to_owned(),
        records_digest,
        unresolved_threads: 0,
        unaddressed_findings: 0,
        outstanding_change_requests: 0,
    }
    .canonical_digest()
    .expect("feedback snapshot digest");
    let payload = TapReviewCompletionPayload {
        schema: TAP_REVIEW_COMPLETION_SCHEMA.to_owned(),
        repository: candidate.policy.repository.clone(),
        pull_request_number: number,
        head_sha: head.to_owned(),
        verdict: TapReviewVerdict::Complete,
        feedback_digest,
        has_unaddressed_findings: false,
        reviewers: vec![TapCompletedReviewer {
            app_id: REVIEWER_APP,
            head_sha: head.to_owned(),
            evidence_digest: digest_b3(b"evidence"),
            complete: true,
            has_unaddressed_findings: false,
        }],
    };
    let text = String::from_utf8(canonical_json_bytes(&payload).expect("completion payload"))
        .expect("UTF8");
    json!({"check_runs": [
        {"name": "DCO", "app": {"id": DCO_APP}, "status": "completed", "conclusion": "success", "head_sha": head},
        {"name": authority.completion_name, "app": {"id": authority.completion_app_id}, "status": "completed", "conclusion": "success", "head_sha": head, "output": {"text": text}}
    ]})
}

pub(super) fn tree(sha: &str, path: &str, blob: &str) -> Value {
    json!({"sha": sha, "truncated": false, "tree": [{"path": path, "mode": "100644", "type": "blob", "sha": blob}]})
}

pub(super) fn content(bytes: &[u8]) -> Value {
    json!({"encoding": "base64", "content": base64::engine::general_purpose::STANDARD.encode(bytes)})
}

pub(super) fn content_route(repository: &str, formula: &str, sha: &str) -> String {
    format!("repos/{repository}/contents/{formula}?ref={sha}")
}

pub(super) fn threads() -> Value {
    json!({"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": [], "pageInfo": {"hasNextPage": false}}}}}})
}

pub(super) fn opinions() -> Value {
    json!({"data": {"repository": {"pullRequest": {"latestOpinionatedReviews": {"nodes": [], "pageInfo": {"hasNextPage": false}}}}}})
}

pub(super) fn put(map: &mut BTreeMap<String, Value>, key: String, value: Value) {
    map.insert(key, value);
}

pub(super) fn request_key(args: &[OsString]) -> String {
    let first = args
        .first()
        .and_then(|arg| arg.to_str())
        .unwrap_or_default();
    if first != "graphql" {
        return first.to_owned();
    }
    let query = args
        .iter()
        .filter_map(|arg| arg.to_str())
        .find(|arg| arg.starts_with("query="))
        .unwrap_or_default();
    let number = args
        .iter()
        .filter_map(|arg| arg.to_str())
        .find_map(|arg| arg.strip_prefix("number="))
        .unwrap_or("0");
    if query.contains("reviewThreads") {
        format!("graphql:threads:{number}")
    } else {
        format!("graphql:opinions:{number}")
    }
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    const HEX: &[u8; 16] = b"0123456789abcdef";
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            output.push(char::from(HEX[usize::from(byte >> 4)]));
            output.push(char::from(HEX[usize::from(byte & 0x0f)]));
            output
        })
}
