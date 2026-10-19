//! Complete, paginated pull-request feedback snapshot and trusted completion.

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
const OPINIONS_QUERY: &str = "query($owner:String!,$name:String!,$number:Int!,$cursor:String){repository(owner:$owner,name:$name){pullRequest(number:$number){latestOpinionatedReviews(first:100,after:$cursor){nodes{id state submittedAt author{__typename ... on User{databaseId} ... on Bot{databaseId}}} pageInfo{hasNextPage endCursor}}}}}";

#[derive(serde::Serialize)]
pub(super) struct FeedbackRecords {
    pub(super) reviews: Vec<Value>,
    pub(super) opinionated_reviews: Vec<Value>,
    pub(super) review_comments: Vec<Value>,
    pub(super) issue_comments: Vec<Value>,
    pub(super) commit_comments: Vec<Value>,
    pub(super) threads: Vec<Value>,
}

pub(super) struct CapturedFeedback {
    pub(super) snapshot: TapFeedbackSnapshot,
    pub(super) completion: Option<TapReviewCompletion>,
    pub(super) records: FeedbackRecords,
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
    let commits = commit_inventory::inventory(api, context)?;
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
    let commit_comments = commit_inventory::comments(api, context.repository, &commits)?;
    let threads = paged_threads(api, context.repository, context.pull_request_number)?;
    let opinionated_reviews =
        paged_opinionated_reviews(api, context.repository, context.pull_request_number)?;
    let outstanding_change_requests = outstanding_change_requests(&opinionated_reviews)?;
    let unresolved_threads = u64::try_from(
        threads
            .iter()
            .filter(|thread| thread["isResolved"].as_bool() == Some(false))
            .count(),
    )
    .map_err(|_| internal("tap_merge_thread_count"))?;
    let records = FeedbackRecords {
        reviews,
        opinionated_reviews,
        review_comments,
        issue_comments,
        commit_comments,
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
    commit_inventory::require_head(api, context, commits.len())?;
    let completion = completion_from_checks(check_runs, context, &snapshot)?;
    Ok(CapturedFeedback {
        snapshot,
        completion,
        records,
    })
}

/// Exact per-commit comment admission is shared by the complete PR collector.
/// Native writer candidates still independently require one commit.
fn paged_commit_comments(
    api: &TapMergeGitHub,
    repository: &str,
    head_sha: &str,
) -> Result<Vec<Value>, OrchestratorError> {
    let comments = paged_values(
        api,
        &format!("repos/{repository}/commits/{head_sha}/comments"),
    )?;
    let mut ids = std::collections::BTreeSet::new();
    for comment in &comments {
        let id = comment["id"]
            .as_u64()
            .filter(|id| *id > 0)
            .ok_or_else(|| internal("tap_merge_commit_comment_identity"))?;
        if comment["commit_id"].as_str() != Some(head_sha) || !ids.insert(id) {
            return Err(internal("tap_merge_commit_comment_identity"));
        }
    }
    Ok(comments)
}

fn paged_opinionated_reviews(
    api: &TapMergeGitHub,
    repository: &str,
    number: u64,
) -> Result<Vec<Value>, OrchestratorError> {
    let (owner, name) = repository
        .split_once('/')
        .ok_or_else(|| internal("tap_merge_repository_invalid"))?;
    let mut cursor = String::new();
    let mut reviews = Vec::new();
    for _ in 0..MAX_PAGES {
        let mut fields = vec![
            ("owner", owner.to_owned()),
            ("name", name.to_owned()),
            ("number", number.to_string()),
        ];
        if !cursor.is_empty() {
            fields.push(("cursor", cursor.clone()));
        }
        let response: Value = api.graphql(OPINIONS_QUERY, &fields)?;
        if response.get("errors").is_some() {
            return Err(internal("tap_merge_opinions_query_failed"));
        }
        let connection = response
            .pointer("/data/repository/pullRequest/latestOpinionatedReviews")
            .ok_or_else(|| internal("tap_merge_opinions_missing"))?;
        let nodes = connection["nodes"]
            .as_array()
            .ok_or_else(|| internal("tap_merge_opinions_invalid"))?;
        if nodes.len() > PAGE_LIMIT as usize
            || reviews.len().saturating_add(nodes.len()) > MAX_FEEDBACK_RECORDS
        {
            return Err(internal("tap_merge_opinions_invalid"));
        }
        reviews.extend(nodes.iter().cloned());
        let page_info = connection
            .get("pageInfo")
            .ok_or_else(|| internal("tap_merge_opinions_page_info"))?;
        if page_info["hasNextPage"].as_bool() == Some(false) {
            validate_opinion_authors(&reviews)?;
            return Ok(reviews);
        }
        if page_info["hasNextPage"].as_bool() != Some(true) {
            return Err(internal("tap_merge_opinions_page_info"));
        }
        page_info["endCursor"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| internal("tap_merge_opinions_cursor"))?
            .clone_into(&mut cursor);
    }
    Err(internal("tap_merge_opinions_page_limit"))
}

fn validate_opinion_authors(reviews: &[Value]) -> Result<(), OrchestratorError> {
    let mut author_ids = std::collections::BTreeSet::new();
    for review in reviews {
        let author = review
            .pointer("/author/databaseId")
            .and_then(Value::as_u64)
            .filter(|id| *id > 0)
            .ok_or_else(|| internal("tap_merge_opinion_author_missing"))?;
        let state = review["state"]
            .as_str()
            .ok_or_else(|| internal("tap_merge_opinion_state_missing"))?;
        if !matches!(state, "APPROVED" | "CHANGES_REQUESTED") || !author_ids.insert(author) {
            return Err(internal("tap_merge_opinion_state_or_author_invalid"));
        }
    }
    Ok(())
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
    validate_opinion_authors(reviews)?;
    u64::try_from(
        reviews
            .iter()
            .filter(|review| review["state"].as_str() == Some("CHANGES_REQUESTED"))
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

#[cfg(test)]
#[path = "tap_merge_feedback_commit_tests.rs"]
mod commit_tests;

#[path = "tap_merge_feedback_commits.rs"]
mod commit_inventory;
