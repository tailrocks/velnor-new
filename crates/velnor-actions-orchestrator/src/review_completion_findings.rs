// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{FeedbackRecords, TapMergeGitHub, time};
use crate::{OrchestratorError, internal::internal};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const QUERY: &str = "query($id:ID!,$cursor:String){node(id:$id){... on PullRequestReviewThread{id isResolved isOutdated pullRequest{number repository{nameWithOwner}} comments(first:100,after:$cursor){nodes{id fullDatabaseId body updatedAt lastEditedAt author{__typename ... on Bot{databaseId}} editor{__typename ... on Bot{databaseId}} pullRequestReview{fullDatabaseId}} pageInfo{hasNextPage endCursor}}}}}";

pub(super) struct Findings {
    pub(super) latest: Option<time::Timestamp>,
    pub(super) metadata: Vec<Value>,
}

pub(super) fn authenticate(
    api: &TapMergeGitHub,
    records: &FeedbackRecords,
    location: (&str, u64),
) -> Result<Findings, OrchestratorError> {
    let mut expected = BTreeMap::new();
    for comment in &records.review_comments {
        let id = positive(comment, "id")?;
        if expected.insert(id, comment).is_some() {
            return Err(internal("review_finding_duplicate"));
        }
    }
    let mut result = Findings {
        latest: None,
        metadata: Vec::new(),
    };
    let mut threads = BTreeSet::new();
    for thread in &records.threads {
        let id = thread["id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| internal("review_finding_thread"))?;
        if thread["isResolved"].as_bool() != Some(true) || !threads.insert(id) {
            return Err(internal("review_finding_unresolved"));
        }
        collect_thread(api, id, records, location, &mut expected, &mut result)?;
    }
    if !expected.is_empty() {
        return Err(internal("review_finding_missing_thread"));
    }
    Ok(result)
}

fn collect_thread(
    api: &TapMergeGitHub,
    id: &str,
    records: &FeedbackRecords,
    location: (&str, u64),
    expected: &mut BTreeMap<u64, &Value>,
    result: &mut Findings,
) -> Result<(), OrchestratorError> {
    let mut cursor = String::new();
    let mut cursors = BTreeSet::new();
    let mut count = 0;
    for _ in 0..100 {
        let mut fields = vec![("id", id.to_owned())];
        if !cursor.is_empty() {
            fields.push(("cursor", cursor.clone()));
        }
        let response: Value = api.graphql(QUERY, &fields)?;
        let node = &response["data"]["node"];
        if response.get("errors").is_some()
            || node["id"].as_str() != Some(id)
            || node["isResolved"].as_bool() != Some(true)
            || node["isOutdated"].as_bool().is_none()
            || node.pointer("/pullRequest/number").and_then(Value::as_u64) != Some(location.1)
            || node
                .pointer("/pullRequest/repository/nameWithOwner")
                .and_then(Value::as_str)
                != Some(location.0)
        {
            return Err(internal("review_finding_thread_identity"));
        }
        let connection = &node["comments"];
        let rows = connection["nodes"]
            .as_array()
            .filter(|rows| rows.len() <= 100)
            .ok_or_else(|| internal("review_finding_comments"))?;
        for row in rows {
            count += 1;
            if count > 20_000 {
                return Err(internal("review_finding_bound"));
            }
            let rest = expected
                .remove(&super::full_database_id(row).ok_or_else(|| internal("review_finding_id"))?)
                .ok_or_else(|| internal("review_finding_comment_bijection"))?;
            let stamp = authenticate_comment(row, rest, records)?;
            result.latest = Some(result.latest.map_or(stamp, |old| old.max(stamp)));
        }
        result.metadata.push(response.clone());
        let page = &connection["pageInfo"];
        if page["hasNextPage"].as_bool() == Some(false) {
            if count == 0 {
                return Err(internal("review_finding_empty_thread"));
            }
            return Ok(());
        }
        let next = page["endCursor"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| internal("review_finding_cursor"))?;
        if page["hasNextPage"].as_bool() != Some(true) || !cursors.insert(next.to_owned()) {
            return Err(internal("review_finding_cursor"));
        }
        next.clone_into(&mut cursor);
    }
    Err(internal("review_finding_page_bound"))
}

fn authenticate_comment(
    node: &Value,
    rest: &Value,
    records: &FeedbackRecords,
) -> Result<time::Timestamp, OrchestratorError> {
    let review = positive(rest, "pull_request_review_id")?;
    let bot = super::PROVIDER_BOT;
    if node["id"] != rest["node_id"]
        || node["body"] != rest["body"]
        || node["updatedAt"] != rest["updated_at"]
        || super::full_database_id(&node["pullRequestReview"]) != Some(review)
        || !records
            .reviews
            .iter()
            .any(|r| r["id"].as_u64() == Some(review))
        || rest.pointer("/user/id").and_then(Value::as_u64) != Some(bot)
        || rest.pointer("/user/type").and_then(Value::as_str) != Some("Bot")
        || node.pointer("/author/databaseId").and_then(Value::as_u64) != Some(bot)
        || node.pointer("/author/__typename").and_then(Value::as_str) != Some("Bot")
        || node.get("editor").is_none()
        || node.get("lastEditedAt").is_none()
    {
        return Err(internal("review_finding_custody"));
    }
    let updated = time::parse(
        rest["updated_at"]
            .as_str()
            .ok_or_else(|| internal("review_finding_time"))?,
    )?;
    let created = time::parse(
        rest["created_at"]
            .as_str()
            .ok_or_else(|| internal("review_finding_time"))?,
    )?;
    if updated < created {
        return Err(internal("review_finding_time"));
    }
    if node["editor"].is_null() && node["lastEditedAt"].is_null() {
        if updated != created {
            return Err(internal("review_finding_editor"));
        }
    } else {
        let edited = time::parse(
            node["lastEditedAt"]
                .as_str()
                .ok_or_else(|| internal("review_finding_editor"))?,
        )?;
        if edited < created
            || edited > updated
            || node.pointer("/editor/databaseId").and_then(Value::as_u64) != Some(bot)
            || node.pointer("/editor/__typename").and_then(Value::as_str) != Some("Bot")
        {
            return Err(internal("review_finding_editor"));
        }
    }
    Ok(updated)
}

fn positive(row: &Value, key: &str) -> Result<u64, OrchestratorError> {
    row[key]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("review_finding_id"))
}
