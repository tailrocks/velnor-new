// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_actions_contract::{
    digest_b3,
    homebrew_merge_gate::{TapFeedbackSnapshot, TapReviewCompletion},
    homebrew_review_completion::{TapCompletionCheckMetadata, parse_tap_review_completion},
};

use crate::{OrchestratorError, internal::internal};

const PAGE_LIMIT: u32 = 100;
const MAX_PAGES: u32 = 100;
const MAX_FEEDBACK_RECORDS: usize = 20_000;
const THREADS_QUERY: &str = "query($owner:String!,$name:String!,$number:Int!,$cursor:String){repository(owner:$owner,name:$name){pullRequest(number:$number){reviewThreads(first:100,after:$cursor){nodes{id isResolved isOutdated} pageInfo{hasNextPage endCursor}}}}}";
const OPINIONS_QUERY: &str = "query($owner:String!,$name:String!,$number:Int!,$cursor:String){repository(owner:$owner,name:$name){pullRequest(number:$number){latestOpinionatedReviews(first:100,after:$cursor){nodes{id state submittedAt author{__typename ... on User{databaseId} ... on Bot{databaseId}}} pageInfo{hasNextPage endCursor}}}}}";

#[derive(Debug, Clone, Copy)]
pub enum FeedbackQuery {
    Threads,
    Opinions,
}

pub trait FeedbackRead {
    fn feedback_get(&self, route: &str) -> Result<Value, OrchestratorError>;
    fn feedback_graphql(
        &self,
        query: FeedbackQuery,
        fields: &[(&str, String)],
    ) -> Result<Value, OrchestratorError>;
}

impl FeedbackQuery {
    pub(crate) fn query(self) -> &'static str {
        match self {
            Self::Threads => THREADS_QUERY,
            Self::Opinions => OPINIONS_QUERY,
        }
    }
}

#[derive(serde::Serialize)]
pub(crate) struct FeedbackRecords {
    pub(crate) reviews: Vec<Value>,
    pub(crate) opinionated_reviews: Vec<Value>,
    pub(crate) review_comments: Vec<Value>,
    pub(crate) issue_comments: Vec<Value>,
    pub(crate) commit_comments: Vec<Value>,
    pub(crate) threads: Vec<Value>,
}

pub(crate) struct CapturedFeedback {
    pub(crate) snapshot: TapFeedbackSnapshot,
    pub(crate) completion: Option<TapReviewCompletion>,
    pub(crate) records: FeedbackRecords,
}

#[derive(Debug)]
pub struct FeedbackContext<'a> {
    pub repository: &'a str,
    pub pull_request_number: u64,
    pub head_sha: &'a str,
    pub completion_app_id: u64,
    pub completion_check_name: &'a str,
    pub expected_reviewer_app_ids: &'a std::collections::BTreeSet<u64>,
}

#[derive(Debug)]
pub(crate) struct FeedbackSubject<'a> {
    pub(crate) repository: &'a str,
    pub(crate) pull_request_number: u64,
    pub(crate) head_sha: &'a str,
}

pub(crate) fn capture_feedback(
    api: &dyn FeedbackRead,
    context: &FeedbackContext<'_>,
    check_runs: &[Value],
) -> Result<CapturedFeedback, OrchestratorError> {
    let subject = FeedbackSubject {
        repository: context.repository,
        pull_request_number: context.pull_request_number,
        head_sha: context.head_sha,
    };
    let mut captured = capture_feedback_subject(api, &subject)?;
    captured.completion = completion_from_checks(check_runs, context, &captured.snapshot)?;
    Ok(captured)
}

pub(crate) fn capture_feedback_subject(
    api: &dyn FeedbackRead,
    context: &FeedbackSubject<'_>,
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
    for records in [&reviews, &review_comments, &issue_comments] {
        pages::validate_numeric_ids(records)?;
    }
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
        unaddressed_findings: 0,
        outstanding_change_requests,
    };
    commit_inventory::require_head(api, context, commits.len())?;
    Ok(CapturedFeedback {
        snapshot,
        completion: None,
        records,
    })
}

pub fn verify_complete_feedback(
    api: &dyn FeedbackRead,
    context: &FeedbackContext<'_>,
    check_runs: &[Value],
) -> Result<TapFeedbackSnapshot, OrchestratorError> {
    let first = capture_feedback(api, context, check_runs)?;
    if first.snapshot.unresolved_threads != 0
        || first.snapshot.outstanding_change_requests != 0
        || first.snapshot.unaddressed_findings != 0
        || first.completion.is_none()
    {
        return Err(internal("qualification_feedback_incomplete"));
    }
    let second = capture_feedback(api, context, check_runs)?;
    if first.snapshot != second.snapshot || first.completion != second.completion {
        return Err(internal("qualification_feedback_changed"));
    }
    Ok(first.snapshot)
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

#[path = "github_review_feedback_commits.rs"]
mod commit_inventory;

#[cfg(test)]
#[path = "../tests/github_review_feedback.rs"]
mod tests;

#[path = "github_review_feedback_pages.rs"]
mod pages;
use pages::{
    paged_commit_comments, paged_opinionated_reviews, paged_threads, paged_values,
    validate_opinion_authors,
};
