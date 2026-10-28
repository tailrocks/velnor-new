// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{FeedbackRecords, PROVIDER_BOT, TapMergeGitHub, authenticate_review, time};
use crate::{OrchestratorError, internal::internal};
use serde_json::Value;
use std::collections::BTreeSet;

const USER_REVIEW_EDITOR: &str = "query($id:ID!){node(id:$id){... on PullRequestReview{fullDatabaseId body submittedAt lastEditedAt commit{oid} author{__typename ... on User{databaseId} ... on Bot{databaseId}} editor{__typename ... on User{databaseId} ... on Bot{databaseId}}}}}";

pub(super) struct Reply {
    pub(super) review_id: u64,
    pub(super) user_id: u64,
    pub(super) created: time::Timestamp,
    pub(super) updated: time::Timestamp,
}

pub(super) fn authenticate_reply(
    node: &Value,
    rest: &Value,
    root: (&Value, &Value),
    records: &FeedbackRecords,
) -> Result<Reply, OrchestratorError> {
    let comment_id = positive(rest, "id")?;
    let user_id = rest
        .pointer("/user/id")
        .and_then(Value::as_u64)
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("review_finding_reply_custody"))?;
    let review_id = positive(rest, "pull_request_review_id")?;
    let review = unique_review(records, review_id)?;
    if node["id"].as_str().is_none()
        || rest["node_id"].as_str() != node["id"].as_str()
        || super::full_database_id(node) != Some(comment_id)
        || rest["body"].as_str().is_none()
        || node["body"] != rest["body"]
        || node["createdAt"] != rest["created_at"]
        || node["updatedAt"] != rest["updated_at"]
        || node.pointer("/author/__typename").and_then(Value::as_str) != Some("User")
        || node.pointer("/author/databaseId").and_then(Value::as_u64) != Some(user_id)
        || super::full_database_id(&node["pullRequestReview"]) != Some(review_id)
        || rest.pointer("/user/type").and_then(Value::as_str) != Some("User")
        || review.pointer("/user/type").and_then(Value::as_str) != Some("User")
        || review.pointer("/user/id").and_then(Value::as_u64) != Some(user_id)
        || review["state"].as_str() != Some("COMMENTED")
        || review["body"].as_str() != Some("")
        || rest["in_reply_to_id"].as_u64() != Some(positive(root.1, "id")?)
        || node.pointer("/replyTo/id") != root.0.get("id")
        || super::full_database_id(&node["replyTo"]) != Some(positive(root.1, "id")?)
        || node
            .pointer("/replyTo/author/__typename")
            .and_then(Value::as_str)
            != Some("Bot")
        || node
            .pointer("/replyTo/author/databaseId")
            .and_then(Value::as_u64)
            != Some(PROVIDER_BOT)
    {
        return Err(internal("review_finding_reply_custody"));
    }
    let created = time::parse(
        rest["created_at"]
            .as_str()
            .ok_or_else(|| internal("review_finding_time"))?,
    )?;
    let updated = time::parse(
        rest["updated_at"]
            .as_str()
            .ok_or_else(|| internal("review_finding_time"))?,
    )?;
    if updated < created {
        return Err(internal("review_finding_time"));
    }
    authenticate_reply_editor(node, user_id, created, updated)?;
    Ok(Reply {
        review_id,
        user_id,
        created,
        updated,
    })
}

fn authenticate_reply_editor(
    node: &Value,
    user_id: u64,
    created: time::Timestamp,
    updated: time::Timestamp,
) -> Result<(), OrchestratorError> {
    if node.get("lastEditedAt").is_none() || node.get("editor").is_none() {
        return Err(internal("review_finding_editor"));
    }
    if node["lastEditedAt"].is_null() && node["editor"].is_null() {
        return (updated == created)
            .then_some(())
            .ok_or_else(|| internal("review_finding_editor"));
    }
    let edited = time::parse(
        node["lastEditedAt"]
            .as_str()
            .ok_or_else(|| internal("review_finding_editor"))?,
    )?;
    if edited < created
        || edited > updated
        || node.pointer("/editor/__typename").and_then(Value::as_str) != Some("User")
        || node.pointer("/editor/databaseId").and_then(Value::as_u64) != Some(user_id)
    {
        return Err(internal("review_finding_editor"));
    }
    Ok(())
}

pub(super) fn authenticate_review_container(
    api: &TapMergeGitHub,
    review: &Value,
    user_id: u64,
    latest_reply: time::Timestamp,
) -> Result<(Value, time::Timestamp), OrchestratorError> {
    let submitted = time::parse(
        review["submitted_at"]
            .as_str()
            .ok_or_else(|| internal("review_finding_review_time"))?,
    )?;
    let node_id = review["node_id"]
        .as_str()
        .ok_or_else(|| internal("review_finding_review_node"))?;
    let response: Value = api.graphql(USER_REVIEW_EDITOR, &[("id", node_id.to_owned())])?;
    let node = &response["data"]["node"];
    let id = positive(review, "id")?;
    let commit = review["commit_id"].as_str();
    if response.get("errors").is_some()
        || super::full_database_id(node) != Some(id)
        || node["body"] != review["body"]
        || node["submittedAt"] != review["submitted_at"]
        || !commit.is_some_and(valid_sha)
        || node.pointer("/commit/oid").and_then(Value::as_str) != commit
        || node.pointer("/author/__typename").and_then(Value::as_str) != Some("User")
        || node.pointer("/author/databaseId").and_then(Value::as_u64) != Some(user_id)
        || submitted < latest_reply
        || node.get("lastEditedAt").is_none()
        || node.get("editor").is_none()
    {
        return Err(internal("review_finding_review_editor"));
    }
    let activity = authenticate_review_editor(node, user_id, submitted)?;
    Ok((response, activity))
}

fn authenticate_review_editor(
    node: &Value,
    user_id: u64,
    submitted: time::Timestamp,
) -> Result<time::Timestamp, OrchestratorError> {
    if node["lastEditedAt"].is_null() && node["editor"].is_null() {
        return Ok(submitted);
    }
    let edited = time::parse(
        node["lastEditedAt"]
            .as_str()
            .ok_or_else(|| internal("review_finding_review_editor"))?,
    )?;
    if edited < submitted
        || node.pointer("/editor/__typename").and_then(Value::as_str) != Some("User")
        || node.pointer("/editor/databaseId").and_then(Value::as_u64) != Some(user_id)
    {
        return Err(internal("review_finding_review_editor"));
    }
    Ok(edited)
}

fn unique_review(records: &FeedbackRecords, id: u64) -> Result<&Value, OrchestratorError> {
    let mut rows = records
        .reviews
        .iter()
        .filter(|review| review["id"].as_u64() == Some(id));
    let review = rows
        .next()
        .ok_or_else(|| internal("review_finding_review_missing"))?;
    if rows.next().is_some() {
        return Err(internal("review_finding_review_duplicate"));
    }
    Ok(review)
}

fn positive(row: &Value, key: &str) -> Result<u64, OrchestratorError> {
    row[key]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("review_finding_id"))
}

fn valid_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

pub(super) fn authenticate_reviews(
    api: &TapMergeGitHub,
    records: &FeedbackRecords,
    human_review_ids: &BTreeSet<u64>,
    terminal: (time::Timestamp, time::Timestamp),
) -> Result<Option<(bool, Vec<Value>)>, OrchestratorError> {
    let mut covered = true;
    let mut ids = BTreeSet::new();
    let mut metadata = Vec::new();
    for review in &records.reviews {
        let id = positive(review, "id")?;
        if !ids.insert(id) {
            return Ok(None);
        }
        if review.pointer("/user/type").and_then(Value::as_str) == Some("User") {
            if human_review_ids.contains(&id) {
                continue;
            }
            return Ok(None);
        }
        if human_review_ids.contains(&id)
            || review.pointer("/user/type").and_then(Value::as_str) != Some("Bot")
            || review.pointer("/user/id").and_then(Value::as_u64) != Some(PROVIDER_BOT)
            || review["state"].as_str() != Some("COMMENTED")
            || !review["body"]
                .as_str()
                .is_some_and(|body| body.starts_with("<!-- codex-review-receipt:v1 -->"))
        {
            return Ok(None);
        }
        let code = receipt_is_code(
            review["body"]
                .as_str()
                .ok_or_else(|| internal("review_provider_receipt"))?,
        )?;
        let (response, activity) = authenticate_review(api, review)?;
        covered &= activity <= if code { terminal.0 } else { terminal.1 };
        metadata.push(response);
    }
    Ok(Some((covered, metadata)))
}

fn receipt_is_code(body: &str) -> Result<bool, OrchestratorError> {
    if body.lines().next() != Some("<!-- codex-review-receipt:v1 -->") {
        return Err(internal("review_provider_receipt"));
    }
    let code = body
        .lines()
        .filter(|line| *line == "### 💡 Codex Review")
        .count();
    let security = body
        .lines()
        .filter(|line| *line == "### 🛡️ Codex Security Review")
        .count();
    match (code, security) {
        (1, 0) => Ok(true),
        (0, 1) => Ok(false),
        _ => Err(internal("review_provider_receipt_role")),
    }
}
