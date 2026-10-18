//! Complete, paginated pull-request feedback snapshot and trusted completion.

use std::collections::BTreeMap;

use serde_json::Value;
use velnor_actions_contract::{
    digest_b3,
    homebrew_merge_gate::{TapFeedbackSnapshot, TapReviewCompletion},
    homebrew_review_completion::{TapCompletionCheckMetadata, parse_tap_review_completion},
};

use crate::{OrchestratorError, internal::internal};

use super::github::TapMergeGitHub;

const PAGE_LIMIT: u32 = 100;
const MAX_PAGES: u32 = 100;
const MAX_FEEDBACK_RECORDS: usize = 20_000;
const THREADS_QUERY: &str = "query($owner:String!,$name:String!,$number:Int!,$cursor:String){repository(owner:$owner,name:$name){pullRequest(number:$number){reviewThreads(first:100,after:$cursor){nodes{id isResolved isOutdated} pageInfo{hasNextPage endCursor}}}}}";

#[derive(serde::Serialize)]
struct FeedbackRecords {
    reviews: Vec<Value>,
    review_comments: Vec<Value>,
    issue_comments: Vec<Value>,
    threads: Vec<Value>,
}

pub(super) struct CapturedFeedback {
    pub(super) snapshot: TapFeedbackSnapshot,
    pub(super) completion: Option<TapReviewCompletion>,
}

pub(super) struct FeedbackContext<'a> {
    pub(super) repository: &'a str,
    pub(super) pull_request_number: u64,
    pub(super) head_sha: &'a str,
    pub(super) completion_app_id: u64,
    pub(super) completion_check_name: &'a str,
    pub(super) expected_reviewer_app_ids: &'a std::collections::BTreeSet<u64>,
}

pub(super) fn capture_feedback(
    api: &TapMergeGitHub,
    context: &FeedbackContext<'_>,
    check_runs: &[Value],
) -> Result<CapturedFeedback, OrchestratorError> {
    let reviews = paged_values(
        api,
        &format!(
            "repos/{}/pulls/{}/reviews",
            context.repository, context.pull_request_number
        ),
    )?;
    let review_comments = paged_values(
        api,
        &format!(
            "repos/{}/pulls/{}/comments",
            context.repository, context.pull_request_number
        ),
    )?;
    let issue_comments = paged_values(
        api,
        &format!(
            "repos/{}/issues/{}/comments",
            context.repository, context.pull_request_number
        ),
    )?;
    let threads = paged_threads(api, context.repository, context.pull_request_number)?;
    let outstanding_change_requests = outstanding_change_requests(&reviews)?;
    let unresolved_threads = u64::try_from(
        threads
            .iter()
            .filter(|thread| thread["isResolved"].as_bool() == Some(false))
            .count(),
    )
    .map_err(|_| internal("tap_merge_thread_count"))?;
    let records = FeedbackRecords {
        reviews,
        review_comments,
        issue_comments,
        threads,
    };
    let records_bytes = velnor_actions_contract::canonical_json_bytes(&records)
        .map_err(|_| internal("tap_merge_feedback_encode"))?;
    let snapshot = TapFeedbackSnapshot {
        repository: context.repository.to_owned(),
        pull_request_number: context.pull_request_number,
        head_sha: context.head_sha.to_owned(),
        records_digest: digest_b3(&records_bytes),
        unresolved_threads,
        // Finding disposition is exclusively asserted by the independently
        // authenticated completion App's closed payload, never free-form text.
        unaddressed_findings: 0,
        outstanding_change_requests,
    };
    let completion = completion_from_checks(check_runs, context, &snapshot)?;
    Ok(CapturedFeedback {
        snapshot,
        completion,
    })
}

fn paged_values(api: &TapMergeGitHub, route: &str) -> Result<Vec<Value>, OrchestratorError> {
    let mut records = Vec::new();
    for page in 1..=MAX_PAGES {
        let response: Vec<Value> =
            api.get(&format!("{route}?per_page={PAGE_LIMIT}&page={page}"))?;
        if response.len() > PAGE_LIMIT as usize
            || records.len().saturating_add(response.len()) > MAX_FEEDBACK_RECORDS
        {
            return Err(internal("tap_merge_feedback_page_limit"));
        }
        let page_len = response.len();
        records.extend(response);
        if page_len < PAGE_LIMIT as usize {
            return Ok(records);
        }
    }
    Err(internal("tap_merge_feedback_page_limit"))
}

fn paged_threads(
    api: &TapMergeGitHub,
    repository: &str,
    number: u64,
) -> Result<Vec<Value>, OrchestratorError> {
    let (owner, name) = repository
        .split_once('/')
        .ok_or_else(|| internal("tap_merge_repository_invalid"))?;
    let mut cursor = String::new();
    let mut threads = Vec::new();
    for _ in 0..MAX_PAGES {
        let mut fields = vec![
            ("owner", owner.to_owned()),
            ("name", name.to_owned()),
            ("number", number.to_string()),
        ];
        if !cursor.is_empty() {
            fields.push(("cursor", cursor.clone()));
        }
        let response: Value = api.graphql(THREADS_QUERY, &fields)?;
        if response.get("errors").is_some() {
            return Err(internal("tap_merge_threads_query_failed"));
        }
        let connection = response
            .pointer("/data/repository/pullRequest/reviewThreads")
            .ok_or_else(|| internal("tap_merge_threads_missing"))?;
        let nodes = connection["nodes"]
            .as_array()
            .ok_or_else(|| internal("tap_merge_threads_invalid"))?;
        if nodes.len() > PAGE_LIMIT as usize
            || threads.len().saturating_add(nodes.len()) > MAX_FEEDBACK_RECORDS
            || nodes.iter().any(|node| {
                node["id"].as_str().is_none()
                    || node["isResolved"].as_bool().is_none()
                    || node["isOutdated"].as_bool().is_none()
            })
        {
            return Err(internal("tap_merge_threads_invalid"));
        }
        threads.extend(nodes.iter().cloned());
        let page_info = connection
            .get("pageInfo")
            .ok_or_else(|| internal("tap_merge_threads_page_info"))?;
        if page_info["hasNextPage"].as_bool() == Some(false) {
            return Ok(threads);
        }
        if page_info["hasNextPage"].as_bool() != Some(true) {
            return Err(internal("tap_merge_threads_page_info"));
        }
        page_info["endCursor"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| internal("tap_merge_threads_cursor"))?
            .clone_into(&mut cursor);
    }
    Err(internal("tap_merge_threads_page_limit"))
}

fn outstanding_change_requests(reviews: &[Value]) -> Result<u64, OrchestratorError> {
    let mut latest = BTreeMap::<u64, (String, u64, String)>::new();
    for review in reviews {
        let user_id = review
            .pointer("/user/id")
            .and_then(Value::as_u64)
            .filter(|id| *id > 0)
            .ok_or_else(|| internal("tap_merge_review_author_missing"))?;
        let review_id = review["id"]
            .as_u64()
            .filter(|id| *id > 0)
            .ok_or_else(|| internal("tap_merge_review_id_missing"))?;
        let state = review["state"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| internal("tap_merge_review_state_missing"))?
            .to_ascii_uppercase();
        if state == "PENDING" {
            continue;
        }
        let submitted = review["submitted_at"]
            .as_str()
            .ok_or_else(|| internal("tap_merge_review_time_missing"))?;
        let candidate = (submitted.to_owned(), review_id, state);
        if latest
            .get(&user_id)
            .is_none_or(|existing| (&candidate.0, candidate.1) > (&existing.0, existing.1))
        {
            latest.insert(user_id, candidate);
        }
    }
    u64::try_from(
        latest
            .values()
            .filter(|(_, _, state)| state == "CHANGES_REQUESTED")
            .count(),
    )
    .map_err(|_| internal("tap_merge_review_count"))
}

fn completion_from_checks(
    check_runs: &[Value],
    context: &FeedbackContext<'_>,
    snapshot: &TapFeedbackSnapshot,
) -> Result<Option<TapReviewCompletion>, OrchestratorError> {
    let matching = check_runs
        .iter()
        .filter(|run| run["name"].as_str() == Some(context.completion_check_name))
        .filter(|run| {
            run.pointer("/app/id").and_then(Value::as_u64) == Some(context.completion_app_id)
        })
        .collect::<Vec<_>>();
    if matching.is_empty() {
        return Ok(None);
    }
    if matching.len() != 1 {
        return Err(internal("tap_merge_completion_check_ambiguous"));
    }
    let run = matching[0];
    if run["status"].as_str() != Some("completed") {
        return Ok(None);
    }
    if run["conclusion"].as_str() != Some("success")
        || run["head_sha"].as_str() != Some(context.head_sha)
    {
        return Err(internal("tap_merge_completion_check_failed"));
    }
    let metadata = TapCompletionCheckMetadata {
        name: context.completion_check_name.to_owned(),
        app_id: context.completion_app_id,
        head_sha: run["head_sha"]
            .as_str()
            .ok_or_else(|| internal("tap_merge_completion_head_missing"))?
            .to_owned(),
        completed_successfully: true,
    };
    let text = run
        .pointer("/output/text")
        .and_then(Value::as_str)
        .ok_or_else(|| internal("tap_merge_completion_output_missing"))?;
    let parsed = parse_tap_review_completion(&metadata, text.as_bytes())
        .map_err(|_| internal("tap_merge_completion_payload_invalid"))?;
    if parsed.repository != context.repository
        || parsed.pull_request_number != context.pull_request_number
        || parsed.completed_reviewer_app_ids != *context.expected_reviewer_app_ids
        || parsed.feedback_digest
            != snapshot
                .canonical_digest()
                .map_err(|_| internal("tap_merge_feedback_digest"))?
    {
        return Err(internal("tap_merge_completion_binding_mismatch"));
    }
    Ok(Some(parsed))
}

#[cfg(test)]
#[path = "tap_merge_feedback_tests.rs"]
mod tests;
