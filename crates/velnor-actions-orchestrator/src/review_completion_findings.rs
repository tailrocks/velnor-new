// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{FeedbackRecords, TapMergeGitHub, dispositions, time};
use crate::{OrchestratorError, internal::internal};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const QUERY: &str = "query($id:ID!,$cursor:String){node(id:$id){... on PullRequestReviewThread{id isResolved isOutdated pullRequest{number repository{nameWithOwner}} comments(first:100,after:$cursor){nodes{id fullDatabaseId body createdAt updatedAt lastEditedAt author{__typename ... on User{databaseId} ... on Bot{databaseId}} editor{__typename ... on User{databaseId} ... on Bot{databaseId}} pullRequestReview{fullDatabaseId} replyTo{id fullDatabaseId author{__typename ... on User{databaseId} ... on Bot{databaseId}}}} pageInfo{hasNextPage endCursor}}}}}";

pub(super) struct Findings {
    pub(super) latest: Option<time::Timestamp>,
    pub(super) metadata: Vec<Value>,
    pub(super) human_review_ids: BTreeSet<u64>,
    human_reviews: BTreeMap<u64, HumanReview>,
}

struct HumanReview {
    user_id: u64,
    latest_reply: time::Timestamp,
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
        human_review_ids: BTreeSet::new(),
        human_reviews: BTreeMap::new(),
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
    authenticate_human_reviews(api, records, &mut result)?;
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
    let mut provider_comments = Vec::new();
    let mut replies = Vec::new();
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
            let id = super::full_database_id(row).ok_or_else(|| internal("review_finding_id"))?;
            let rest = expected
                .remove(&id)
                .ok_or_else(|| internal("review_finding_comment_bijection"))?;
            match row.pointer("/author/__typename").and_then(Value::as_str) {
                Some("Bot") => provider_comments.push((row.clone(), rest)),
                Some("User") => replies.push((row.clone(), rest)),
                _ => return Err(internal("review_finding_comment_author")),
            }
        }
        result.metadata.push(response.clone());
        let page = &connection["pageInfo"];
        if page["hasNextPage"].as_bool() == Some(false) {
            if count == 0 {
                return Err(internal("review_finding_empty_thread"));
            }
            return authenticate_thread(provider_comments, replies, records, result);
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

fn authenticate_thread(
    mut provider_comments: Vec<(Value, &Value)>,
    replies: Vec<(Value, &Value)>,
    records: &FeedbackRecords,
    result: &mut Findings,
) -> Result<(), OrchestratorError> {
    let roots = provider_comments
        .iter()
        .enumerate()
        .filter(|(_, (node, rest))| node["replyTo"].is_null() && rest["in_reply_to_id"].is_null())
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if roots.len() != 1 {
        return Err(internal("review_finding_root"));
    }
    let (root_node, root_rest) = provider_comments.remove(roots[0]);
    let stamp = authenticate_comment(&root_node, root_rest, records, None)?;
    result.latest = Some(result.latest.map_or(stamp, |old| old.max(stamp)));
    for (node, rest) in provider_comments {
        let stamp = authenticate_comment(&node, rest, records, Some((&root_node, root_rest)))?;
        result.latest = Some(result.latest.map_or(stamp, |old| old.max(stamp)));
    }
    for (node, rest) in replies {
        let reply =
            dispositions::authenticate_reply(&node, rest, (&root_node, root_rest), records)?;
        let group = result
            .human_reviews
            .entry(reply.review_id)
            .or_insert(HumanReview {
                user_id: reply.user_id,
                latest_reply: reply.created,
            });
        if group.user_id != reply.user_id {
            return Err(internal("review_finding_review_identity"));
        }
        group.latest_reply = group.latest_reply.max(reply.created);
        let latest = reply.updated;
        result.latest = Some(result.latest.map_or(latest, |old| old.max(latest)));
    }
    Ok(())
}

fn authenticate_human_reviews(
    api: &TapMergeGitHub,
    records: &FeedbackRecords,
    result: &mut Findings,
) -> Result<(), OrchestratorError> {
    for review in &records.reviews {
        if review.pointer("/user/type").and_then(Value::as_str) != Some("User") {
            continue;
        }
        let id = positive(review, "id")?;
        let group = result
            .human_reviews
            .remove(&id)
            .ok_or_else(|| internal("review_finding_review_orphan"))?;
        if review.pointer("/user/id").and_then(Value::as_u64) != Some(group.user_id)
            || review["state"].as_str() != Some("COMMENTED")
            || review["body"].as_str() != Some("")
        {
            return Err(internal("review_finding_review_identity"));
        }
        let (metadata, activity) = dispositions::authenticate_review_container(
            api,
            review,
            group.user_id,
            group.latest_reply,
        )?;
        result.human_review_ids.insert(id);
        result.metadata.push(metadata);
        result.latest = Some(result.latest.map_or(activity, |old| old.max(activity)));
    }
    if result.human_reviews.is_empty() {
        Ok(())
    } else {
        Err(internal("review_finding_review_missing"))
    }
}

fn authenticate_comment(
    node: &Value,
    rest: &Value,
    records: &FeedbackRecords,
    root: Option<(&Value, &Value)>,
) -> Result<time::Timestamp, OrchestratorError> {
    let review = positive(rest, "pull_request_review_id")?;
    let comment_id = positive(rest, "id")?;
    let bot = super::PROVIDER_BOT;
    let matching_reviews = records
        .reviews
        .iter()
        .filter(|row| row["id"].as_u64() == Some(review));
    let matching_reviews = matching_reviews.collect::<Vec<_>>();
    let review_record = matching_reviews
        .first()
        .copied()
        .ok_or_else(|| internal("review_finding_review_missing"))?;
    let parent_is_valid = comment_parent_valid(node, rest, root, bot)?;
    if node["id"].as_str().is_none()
        || rest["node_id"].as_str() != node["id"].as_str()
        || super::full_database_id(node) != Some(comment_id)
        || rest["body"].as_str().is_none()
        || node["body"] != rest["body"]
        || node["createdAt"] != rest["created_at"]
        || node["updatedAt"] != rest["updated_at"]
        || super::full_database_id(&node["pullRequestReview"]) != Some(review)
        || matching_reviews.len() != 1
        || review_record.pointer("/user/id").and_then(Value::as_u64) != Some(bot)
        || review_record.pointer("/user/type").and_then(Value::as_str) != Some("Bot")
        || review_record["state"].as_str() != Some("COMMENTED")
        || !review_record["body"]
            .as_str()
            .is_some_and(|body| body.starts_with("<!-- codex-review-receipt:v1 -->"))
        || rest.pointer("/user/id").and_then(Value::as_u64) != Some(bot)
        || rest.pointer("/user/type").and_then(Value::as_str) != Some("Bot")
        || node.pointer("/author/databaseId").and_then(Value::as_u64) != Some(bot)
        || node.pointer("/author/__typename").and_then(Value::as_str) != Some("Bot")
        || !parent_is_valid
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

fn comment_parent_valid(
    node: &Value,
    rest: &Value,
    root: Option<(&Value, &Value)>,
    bot: u64,
) -> Result<bool, OrchestratorError> {
    match root {
        None => {
            Ok(rest["in_reply_to_id"].is_null() && node.get("replyTo").is_some_and(Value::is_null))
        }
        Some((root_node, root_rest)) => {
            let root_id = positive(root_rest, "id")?;
            Ok(rest["in_reply_to_id"].as_u64() == Some(root_id)
                && node.pointer("/replyTo/id") == root_node.get("id")
                && super::full_database_id(&node["replyTo"]) == Some(root_id)
                && node
                    .pointer("/replyTo/author/__typename")
                    .and_then(Value::as_str)
                    == Some("Bot")
                && node
                    .pointer("/replyTo/author/databaseId")
                    .and_then(Value::as_u64)
                    == Some(bot))
        }
    }
}

fn positive(row: &Value, key: &str) -> Result<u64, OrchestratorError> {
    row[key]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("review_finding_id"))
}
