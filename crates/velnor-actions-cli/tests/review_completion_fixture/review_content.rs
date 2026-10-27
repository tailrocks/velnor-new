// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::Scenario;
use super::{HEAD, NUMBER, REPOSITORY};
use serde_json::{Map, Value, json};
use std::{error::Error, path::Path};

const REVIEW_NODE_ID: &str = "PRR_codex_review";
const THREAD_NODE_ID: &str = "PRRT_codex_inline";
const COMMENT_NODE_ID: &str = "PRRC_codex_inline";
const WIDE_REVIEW_ID: u64 = 5_672_229_398;
const WIDE_COMMENT_ID: u64 = 4_392_518_451;
const INLINE_REVIEW_ID: u64 = 6_502;
const REVIEW_BODY: &str =
    "<!-- codex-review-receipt:v1 -->\n\n### 💡 Codex Review\nThe reviewed head has no findings.";
const COMMENT_BODY: &str = "The inline finding was addressed.";
const REVIEW_TIME: &str = "2026-10-27T14:00:00Z";
const COMMENT_TIME: &str = "2026-10-27T14:10:00Z";

pub(super) fn has_review(scenario: Scenario) -> bool {
    matches!(
        scenario,
        Scenario::ReviewFullDatabaseId
            | Scenario::ReviewMismatchedFullDatabaseId
            | Scenario::InlineFullDatabaseId
            | Scenario::InlineMismatchedParent
            | Scenario::InlineNonCanonicalId
            | Scenario::InlineDuplicateBijection
    )
}

fn has_inline_comment(scenario: Scenario) -> bool {
    matches!(
        scenario,
        Scenario::InlineFullDatabaseId
            | Scenario::InlineMismatchedParent
            | Scenario::InlineNonCanonicalId
            | Scenario::InlineDuplicateBijection
    )
}

pub(super) fn write_responses(state: &Path, scenario: Scenario) -> Result<(), Box<dyn Error>> {
    let review = review_id(scenario);
    let review_rows = if has_review(scenario) {
        vec![review_record(scenario)]
    } else {
        Vec::new()
    };
    let comment_rows = if has_inline_comment(scenario) {
        vec![comment_record()]
    } else {
        Vec::new()
    };
    super::put_json(state, "reviews.json", &Value::Array(review_rows))?;
    super::put_json(state, "review-comments.json", &Value::Array(comment_rows))?;
    super::put_json(state, "threads.json", &thread_inventory(scenario))?;
    super::put_json(
        state,
        "review-editor.json",
        &review_editor(scenario, false, review),
    )?;
    super::put_json(
        state,
        "review-full-editor.json",
        &review_editor(scenario, true, review),
    )?;
    super::put_json(
        state,
        "review-thread-legacy.json",
        &thread_detail(scenario, false, review),
    )?;
    super::put_json(
        state,
        "review-thread-syntax-error.json",
        &json!({"errors":[{"message":"Expected NAME, actual: (none) (\"\") at [1, 376]","locations":[{"line":1,"column":376}]}]}),
    )?;
    super::put_json(
        state,
        "review-thread-full.json",
        &thread_detail(scenario, true, review),
    )?;
    Ok(())
}

fn review_id(scenario: Scenario) -> u64 {
    if has_inline_comment(scenario) {
        INLINE_REVIEW_ID
    } else {
        WIDE_REVIEW_ID
    }
}

fn review_record(scenario: Scenario) -> Value {
    json!({
        "id":review_id(scenario),"node_id":REVIEW_NODE_ID,"user":{"id":199_175_422,"type":"Bot"},
        "state":"COMMENTED","body":REVIEW_BODY,"submitted_at":REVIEW_TIME,"commit_id":HEAD
    })
}

fn review_editor(scenario: Scenario, full: bool, review_id: u64) -> Value {
    let id = if full {
        json!(match scenario {
            Scenario::ReviewMismatchedFullDatabaseId => "5672229399".to_owned(),
            _ => review_id.to_string(),
        })
    } else if review_id > i32::MAX as u64 {
        Value::Null
    } else {
        json!(review_id)
    };
    let key = if full { "fullDatabaseId" } else { "databaseId" };
    let mut response = json!({
        "data":{"node":{
            "body":REVIEW_BODY,"submittedAt":REVIEW_TIME,"lastEditedAt":null,
            "commit":{"oid":HEAD},"author":{"__typename":"Bot","databaseId":199_175_422},"editor":null
        }}
    });
    response["data"]["node"][key] = id;
    if !full && review_id > i32::MAX as u64 {
        response["errors"] =
            json!([{"message":"Int cannot represent this value","path":["node","databaseId"]}]);
    }
    response
}

fn comment_record() -> Value {
    json!({
        "id":WIDE_COMMENT_ID,"node_id":COMMENT_NODE_ID,"pull_request_review_id":INLINE_REVIEW_ID,
        "user":{"id":199_175_422,"type":"Bot"},"body":COMMENT_BODY,
        "created_at":COMMENT_TIME,"updated_at":COMMENT_TIME,"path":"src/lib.rs","line":1
    })
}

fn thread_inventory(scenario: Scenario) -> Value {
    let nodes = if has_inline_comment(scenario) {
        json!([{"id":THREAD_NODE_ID,"isResolved":true,"isOutdated":false}])
    } else {
        json!([])
    };
    json!({"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":nodes,"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}})
}

fn thread_detail(scenario: Scenario, full: bool, review_id: u64) -> Value {
    let parent_id = match scenario {
        Scenario::InlineMismatchedParent => "6503".to_owned(),
        _ => review_id.to_string(),
    };
    let comment_id = match scenario {
        Scenario::InlineNonCanonicalId => "04392518451".to_owned(),
        _ => WIDE_COMMENT_ID.to_string(),
    };
    let comment_id_value = if full { json!(comment_id) } else { Value::Null };
    let parent_id_value = if full {
        json!(parent_id)
    } else {
        json!(review_id)
    };
    let comment_key = if full { "fullDatabaseId" } else { "databaseId" };
    let parent_key = if full { "fullDatabaseId" } else { "databaseId" };
    let mut row = json!({
        "id":COMMENT_NODE_ID,"body":COMMENT_BODY,"updatedAt":COMMENT_TIME,"lastEditedAt":null,
        "author":{"__typename":"Bot","databaseId":199_175_422},"editor":null
    });
    row[comment_key] = comment_id_value;
    let mut parent = Map::new();
    parent.insert(parent_key.to_owned(), parent_id_value);
    row["pullRequestReview"] = Value::Object(parent);
    let rows = if matches!(scenario, Scenario::InlineDuplicateBijection) {
        json!([row.clone(), row])
    } else {
        json!([row])
    };
    let mut response = json!({
        "data":{"node":{
            "id":THREAD_NODE_ID,"isResolved":true,"isOutdated":false,
            "pullRequest":{"number":NUMBER,"repository":{"nameWithOwner":REPOSITORY}},
            "comments":{"nodes":rows,"pageInfo":{"hasNextPage":false,"endCursor":null}}
        }}
    });
    if !full {
        response["errors"] = json!([{"message":"Int cannot represent this value","path":["node","comments","nodes",0,"databaseId"]}]);
    }
    response
}

#[test]
fn codex_review_authenticates_canonical_full_database_id() -> Result<(), Box<dyn Error>> {
    super::run(super::super::Scenario::ReviewFullDatabaseId, true, "ready")?;
    super::run(
        super::super::Scenario::ReviewMismatchedFullDatabaseId,
        false,
        "blocked",
    )
}

#[test]
fn inline_review_comment_binds_canonical_ids_to_its_review() -> Result<(), Box<dyn Error>> {
    for (scenario, success, state) in [
        (super::super::Scenario::InlineFullDatabaseId, true, "ready"),
        (
            super::super::Scenario::InlineMismatchedParent,
            false,
            "blocked",
        ),
        (
            super::super::Scenario::InlineNonCanonicalId,
            false,
            "blocked",
        ),
        (
            super::super::Scenario::InlineDuplicateBijection,
            false,
            "blocked",
        ),
    ] {
        super::run(scenario, success, state)?;
    }
    Ok(())
}
