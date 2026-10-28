// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::Rows;
use serde_json::{Value, json};
use std::{error::Error, fs, path::Path};

const BOT_ID: u64 = 199_175_422;
const COMMENT_ID: u64 = 4_396_249_200;
const REVIEW_ID: u64 = 6_503;
const REVIEW_NODE_ID: &str = "PRR_codex_followup";
const REVIEW_TIME: &str = "2026-10-27T14:16:00Z";
const CREATED_AT: &str = "2026-10-27T14:15:00Z";
const BODY: &str = "Authenticated provider follow-up.";
const REVIEW_BODY: &str =
    "<!-- codex-review-receipt:v1 -->\n\n### 💡 Codex Review\nAuthenticated follow-up receipt.";

pub(super) fn append(rows: &mut Rows, state: &Path) -> Result<(), Box<dyn Error>> {
    let review_id = REVIEW_ID.to_string();
    let review = json!({
        "id":REVIEW_ID,"node_id":REVIEW_NODE_ID,"user":{"id":BOT_ID,"type":"Bot"},
        "state":"COMMENTED","body":REVIEW_BODY,"submitted_at":REVIEW_TIME,"commit_id":super::super::HEAD
    });
    rows.reviews
        .as_array_mut()
        .ok_or("fixture reviews array absent")?
        .push(review);
    let review_response = json!({"data":{"node":{
        "fullDatabaseId":review_id,"body":REVIEW_BODY,"submittedAt":REVIEW_TIME,
        "lastEditedAt":null,"commit":{"oid":super::super::HEAD},
        "author":{"__typename":"Bot","databaseId":BOT_ID},"editor":null
    }}});
    fs::write(
        state.join("review-followup-editor.json"),
        serde_json::to_vec(&review_response)?,
    )?;
    let node_id = format!("PRRC_codex_followup_{COMMENT_ID}");
    let rest = json!({
        "id":COMMENT_ID,"node_id":node_id,"pull_request_review_id":REVIEW_ID,
        "in_reply_to_id":rows.root_id,"user":{"id":BOT_ID,"type":"Bot"},
        "body":BODY,"created_at":CREATED_AT,"updated_at":CREATED_AT,
        "path":"src/lib.rs","line":1,"commit_id":super::super::HEAD
    });
    let row = json!({
        "id":node_id,"fullDatabaseId":COMMENT_ID.to_string(),"body":BODY,
        "createdAt":CREATED_AT,"updatedAt":CREATED_AT,"lastEditedAt":null,
        "author":{"__typename":"Bot","databaseId":BOT_ID,"login":"codex"},
        "editor":null,"pullRequestReview":{"fullDatabaseId":review_id},
        "replyTo":{"id":rows.root_node,"fullDatabaseId":rows.root_id.to_string(),
            "author":{"__typename":"Bot","databaseId":BOT_ID}}
    });
    rows.comments
        .as_array_mut()
        .ok_or("fixture comments array absent")?
        .push(rest);
    insert_thread_row(&mut rows.legacy, row.clone())?;
    insert_thread_row(&mut rows.expanded, row)?;
    Ok(())
}

pub(super) fn foreign_parent(rows: &mut super::Rows) {
    let foreign_id = rows.root_id + 1;
    rows.comments[1]["in_reply_to_id"] = json!(foreign_id);
    for response in [&mut rows.legacy, &mut rows.expanded] {
        let parent = &mut response["data"]["node"]["comments"]["nodes"][1]["replyTo"];
        parent["id"] = json!("PRRC_foreign_parent");
        parent["fullDatabaseId"] = json!(foreign_id.to_string());
    }
}

fn insert_thread_row(response: &mut Value, row: Value) -> Result<(), Box<dyn Error>> {
    response["data"]["node"]["comments"]["nodes"]
        .as_array_mut()
        .ok_or("thread comments array absent")?
        .insert(1, row);
    Ok(())
}
