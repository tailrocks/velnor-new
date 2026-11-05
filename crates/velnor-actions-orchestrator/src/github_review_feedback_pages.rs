// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    FeedbackQuery, FeedbackRead, MAX_FEEDBACK_RECORDS, MAX_PAGES, OrchestratorError, PAGE_LIMIT,
    Value, internal,
};

pub(super) fn paged_commit_comments(
    api: &dyn FeedbackRead,
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

pub(super) fn paged_opinionated_reviews(
    api: &dyn FeedbackRead,
    repository: &str,
    number: u64,
) -> Result<Vec<Value>, OrchestratorError> {
    let (owner, name) = repository
        .split_once('/')
        .ok_or_else(|| internal("tap_merge_repository_invalid"))?;
    let mut cursor = String::new();
    let mut seen_cursors = std::collections::BTreeSet::new();
    let mut seen_ids = std::collections::BTreeSet::new();
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
        let response: Value = api.feedback_graphql(FeedbackQuery::Opinions, &fields)?;
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
        for node in nodes {
            let id = node["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| internal("tap_merge_opinions_identity"))?;
            if !seen_ids.insert(id.to_owned()) {
                return Err(internal("tap_merge_opinions_identity"));
            }
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
        let next = page_info["endCursor"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| internal("tap_merge_opinions_cursor"))?;
        if nodes.is_empty() || !seen_cursors.insert(next.to_owned()) {
            return Err(internal("tap_merge_opinions_cursor"));
        }
        next.clone_into(&mut cursor);
    }
    Err(internal("tap_merge_opinions_page_limit"))
}

pub(super) fn validate_opinion_authors(reviews: &[Value]) -> Result<(), OrchestratorError> {
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

pub(super) fn paged_values(
    api: &dyn FeedbackRead,
    route: &str,
) -> Result<Vec<Value>, OrchestratorError> {
    let mut records = Vec::new();
    for page in 1..=MAX_PAGES {
        let response: Vec<Value> = serde_json::from_value(
            api.feedback_get(&format!("{route}?per_page={PAGE_LIMIT}&page={page}"))?,
        )
        .map_err(|_| internal("tap_merge_feedback_page_invalid"))?;
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

pub(super) fn paged_threads(
    api: &dyn FeedbackRead,
    repository: &str,
    number: u64,
) -> Result<Vec<Value>, OrchestratorError> {
    let (owner, name) = repository
        .split_once('/')
        .ok_or_else(|| internal("tap_merge_repository_invalid"))?;
    let mut cursor = String::new();
    let mut seen_cursors = std::collections::BTreeSet::new();
    let mut seen_ids = std::collections::BTreeSet::new();
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
        let response: Value = api.feedback_graphql(FeedbackQuery::Threads, &fields)?;
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
        for node in nodes {
            let id = node["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| internal("tap_merge_threads_identity"))?;
            if !seen_ids.insert(id.to_owned()) {
                return Err(internal("tap_merge_threads_identity"));
            }
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
        let next = page_info["endCursor"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| internal("tap_merge_threads_cursor"))?;
        if nodes.is_empty() || !seen_cursors.insert(next.to_owned()) {
            return Err(internal("tap_merge_threads_cursor"));
        }
        next.clone_into(&mut cursor);
    }
    Err(internal("tap_merge_threads_page_limit"))
}

pub(super) fn validate_numeric_ids(records: &[Value]) -> Result<(), OrchestratorError> {
    let mut seen = std::collections::BTreeSet::new();
    for record in records {
        let id = record["id"]
            .as_u64()
            .filter(|id| *id > 0)
            .ok_or_else(|| internal("tap_merge_feedback_identity"))?;
        if !seen.insert(id) {
            return Err(internal("tap_merge_feedback_identity"));
        }
    }
    Ok(())
}
