// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Scenario, assert_completion, assert_success, assert_transport_clean, proposal_digest, read_json,
};
use serde_json::{Value, json};
use std::{error::Error, fs, path::Path};

const USER_ID: u64 = 139_017;
const REPLY_IDS: [u64; 2] = [4_396_249_008, 4_396_249_148];
const REVIEW_IDS: [u64; 2] = [5_676_956_317, 5_676_956_488];
const REPLY_TIMES: [&str; 2] = ["2026-10-27T14:20:00Z", "2026-10-27T14:30:00Z"];
const CONTAINER_TIMES: [&str; 2] = ["2026-10-27T14:25:00Z", "2026-10-27T14:35:00Z"];
const LATE_TIME: &str = "2026-10-27T15:10:00Z";
const HISTORICAL_COMMIT: &str = "59735e14b38d80978aa4995f659a9e7a60c8e872";
const HISTORICAL_REPLY_COMMIT: &str = "c21cd07e3be8b90f637dd7982b2e4e9c15cb373c";

#[path = "bot_followup.rs"]
mod bot_followup;
#[path = "dispositions_journeys.rs"]
mod journeys;

#[derive(Debug, Clone, Copy)]
enum Case {
    OneReply,
    MultipleReplies,
    HistoricalCommit,
    BotFollowup,
    ForeignBotFollowupParent,
    WrongParent,
    MissingParent,
    ForeignRepository,
    ForeignPullRequest,
    OrphanReview,
    NonemptyReview,
    MismatchedAuthor,
    MismatchedEditor,
    MismatchedBody,
    MismatchedTime,
    MismatchedId,
    UnresolvedThread,
    ChangeRequest,
    LateActivity,
    BothRolesLater,
}

fn run(case: Case, success: bool, state: &str) -> Result<(), Box<dyn Error>> {
    let fixture = super::Fixture::new(Scenario::InlineFullDatabaseId)?;
    write_case(&fixture.state, case)?;
    let (proposal, digest) = capture(&fixture)?;
    assert_eq!(proposal["state"], state, "case: {case:?}");
    let emitted = fixture
        .command("emit-review-completion-v1")
        .env("VELNOR_REVIEW_COMPLETION_PROPOSAL_DIGEST", digest)
        .output()?;
    assert_transport_clean(&fixture.state)?;
    assert_success(&emitted, "emit")?;
    let check = read_json(&fixture.state.join("completion-check.json"))?;
    assert_completion(&check, success)?;
    Ok(())
}

fn capture(fixture: &super::Fixture) -> Result<(Value, String), Box<dyn Error>> {
    let output = fixture.command("capture-review-completion-v1").output()?;
    assert_transport_clean(&fixture.state)?;
    assert_success(&output, "capture")?;
    let proposal = read_json(
        &fixture
            .runner
            .join("velnor/review-completion-proposal.json"),
    )?;
    let output = fs::read_to_string(&fixture.output)?;
    Ok((proposal, proposal_digest(&output)?))
}

fn write_case(state: &Path, case: Case) -> Result<(), Box<dyn Error>> {
    let count = match case {
        Case::OrphanReview | Case::BotFollowup | Case::ForeignBotFollowupParent => 0,
        Case::OneReply | Case::HistoricalCommit => 1,
        _ => 2,
    };
    let mut rows = build_rows(state, count, case)?;
    mutate_case(state, case, &mut rows)?;
    save_rows(state, rows)
}

struct Rows {
    reviews: Value,
    comments: Value,
    legacy: Value,
    expanded: Value,
    editors: Vec<Value>,
    root_id: u64,
    root_node: String,
}

fn build_rows(state: &Path, count: usize, case: Case) -> Result<Rows, Box<dyn Error>> {
    let reviews = read_json(&state.join("reviews.json"))?;
    let comments = read_json(&state.join("review-comments.json"))?;
    let legacy = read_json(&state.join("review-thread-full.json"))?;
    let root = legacy["data"]["node"]["comments"]["nodes"][0].clone();
    let root_id = root["fullDatabaseId"]
        .as_str()
        .ok_or("root full ID absent")?
        .parse::<u64>()?;
    let root_node = root["id"].as_str().ok_or("root node ID absent")?.to_owned();
    let mut expanded = legacy.clone();
    expanded["data"]["node"]["comments"]["nodes"][0]["createdAt"] =
        comments[0]["created_at"].clone();
    expanded["data"]["node"]["comments"]["nodes"][0]["replyTo"] = Value::Null;
    let mut rows = Rows {
        reviews,
        comments,
        expanded,
        legacy,
        editors: Vec::new(),
        root_id,
        root_node,
    };
    if matches!(case, Case::BotFollowup | Case::ForeignBotFollowupParent) {
        bot_followup::append(&mut rows, state)?;
    }
    for index in 0..count {
        append_reply(&mut rows, index, case)?;
    }
    if matches!(case, Case::OrphanReview) {
        rows.editors.push(orphan_review());
    }
    Ok(rows)
}

fn append_reply(rows: &mut Rows, index: usize, case: Case) -> Result<(), Box<dyn Error>> {
    let reply_id = REPLY_IDS[index];
    let review_id = REVIEW_IDS[index];
    let (created_at, submitted_at) = if matches!(case, Case::LateActivity | Case::BothRolesLater) {
        (LATE_TIME, "2026-10-27T15:11:00Z")
    } else {
        (REPLY_TIMES[index], CONTAINER_TIMES[index])
    };
    let review_commit = if matches!(case, Case::HistoricalCommit) {
        HISTORICAL_COMMIT
    } else {
        super::HEAD
    };
    let body = format!("Resolved the finding in historical reply {index}.");
    rows.editors.push(json!({
        "id":review_id,"node_id":format!("PRR_human_reply_{index}"),
        "user":{"id":USER_ID,"type":"User"},"state":"COMMENTED","body":"",
        "submitted_at":submitted_at,"commit_id":review_commit
    }));
    rows.comments
        .as_array_mut()
        .ok_or("fixture comments array absent")?
        .push(json!({
            "id":reply_id,"node_id":format!("PRRC_human_reply_{index}"),
            "pull_request_review_id":review_id,"in_reply_to_id":rows.root_id,
            "user":{"id":USER_ID,"type":"User"},"body":body,
            "created_at":created_at,"updated_at":created_at,"path":"src/lib.rs","line":1,
            "commit_id":if matches!(case, Case::HistoricalCommit) {
                HISTORICAL_REPLY_COMMIT
            } else {
                super::HEAD
            }
        }));
    let reply = json!({
        "id":format!("PRRC_human_reply_{index}"),"fullDatabaseId":reply_id.to_string(),
        "body":body,"updatedAt":created_at,"lastEditedAt":null,
        "author":{"__typename":"User"},"editor":null,
        "pullRequestReview":{"fullDatabaseId":review_id.to_string()}
    });
    rows.legacy["data"]["node"]["comments"]["nodes"]
        .as_array_mut()
        .ok_or("legacy comments array absent")?
        .push(reply.clone());
    rows.expanded["data"]["node"]["comments"]["nodes"]
        .as_array_mut()
        .ok_or("expanded comments array absent")?
        .push(json!({
            "id":format!("PRRC_human_reply_{index}"),"fullDatabaseId":reply_id.to_string(),
            "body":body,"createdAt":created_at,"updatedAt":created_at,"lastEditedAt":null,
            "author":{"__typename":"User","databaseId":USER_ID,"login":"donbeave"},
            "editor":null,"pullRequestReview":{"fullDatabaseId":review_id.to_string()},
            "replyTo":{"id":rows.root_node,"fullDatabaseId":rows.root_id.to_string(),
                "author":{"__typename":"Bot","databaseId":199_175_422}}
        }));
    Ok(())
}

fn orphan_review() -> Value {
    json!({
        "id":REVIEW_IDS[0],"node_id":"PRR_human_reply_orphan",
        "user":{"id":USER_ID,"type":"User"},"state":"COMMENTED","body":"",
        "submitted_at":CONTAINER_TIMES[0],"commit_id":super::HEAD
    })
}

fn mutate_case(state: &Path, case: Case, rows: &mut Rows) -> Result<(), Box<dyn Error>> {
    match case {
        Case::WrongParent
        | Case::MissingParent
        | Case::ForeignRepository
        | Case::ForeignPullRequest
        | Case::NonemptyReview
        | Case::MismatchedAuthor
        | Case::MismatchedEditor
        | Case::MismatchedBody
        | Case::MismatchedTime
        | Case::MismatchedId
        | Case::UnresolvedThread => mutate_relationship(state, case, rows),
        Case::ChangeRequest => mutate_change_request(state),
        Case::HistoricalCommit => mutate_historical_commit(state),
        Case::LateActivity | Case::BothRolesLater => mutate_activity(state, case),
        Case::ForeignBotFollowupParent => {
            bot_followup::foreign_parent(rows);
            Ok(())
        }
        Case::OneReply | Case::MultipleReplies | Case::OrphanReview | Case::BotFollowup => Ok(()),
    }
}

fn mutate_relationship(state: &Path, case: Case, rows: &mut Rows) -> Result<(), Box<dyn Error>> {
    match case {
        Case::WrongParent => {
            rows.comments[1]["in_reply_to_id"] = json!(rows.root_id + 1);
            rows.expanded["data"]["node"]["comments"]["nodes"][1]["replyTo"]["fullDatabaseId"] =
                json!((rows.root_id + 1).to_string());
        }
        Case::MissingParent => {
            rows.comments[1]
                .as_object_mut()
                .ok_or("reply not an object")?
                .remove("in_reply_to_id");
            rows.expanded["data"]["node"]["comments"]["nodes"][1]["replyTo"] = Value::Null;
        }
        Case::ForeignRepository => {
            rows.legacy["data"]["node"]["pullRequest"]["repository"]["nameWithOwner"] =
                json!("foreign/repository");
            rows.expanded["data"]["node"]["pullRequest"]["repository"]["nameWithOwner"] =
                json!("foreign/repository");
        }
        Case::ForeignPullRequest => {
            rows.legacy["data"]["node"]["pullRequest"]["number"] = json!(515);
            rows.expanded["data"]["node"]["pullRequest"]["number"] = json!(515);
        }
        Case::NonemptyReview => rows.editors[0]["body"] = json!("I approve this change."),
        Case::MismatchedAuthor => {
            rows.expanded["data"]["node"]["comments"]["nodes"][1]["author"]["databaseId"] =
                json!(USER_ID + 1);
        }
        Case::MismatchedEditor => {
            rows.expanded["data"]["node"]["comments"]["nodes"][1]["editor"] =
                json!({"__typename":"User","databaseId":USER_ID+1,"login":"other"});
            rows.expanded["data"]["node"]["comments"]["nodes"][1]["lastEditedAt"] =
                json!("2026-10-27T14:31:00Z");
            rows.expanded["data"]["node"]["comments"]["nodes"][1]["updatedAt"] =
                json!("2026-10-27T14:31:00Z");
            rows.comments[1]["updated_at"] = json!("2026-10-27T14:31:00Z");
        }
        Case::MismatchedBody => {
            rows.expanded["data"]["node"]["comments"]["nodes"][1]["body"] = json!("different body");
        }
        Case::MismatchedTime => {
            rows.expanded["data"]["node"]["comments"]["nodes"][1]["updatedAt"] =
                json!("2026-10-27T14:21:00Z");
        }
        Case::MismatchedId => {
            rows.expanded["data"]["node"]["comments"]["nodes"][1]["fullDatabaseId"] =
                json!((REPLY_IDS[0] + 1).to_string());
        }
        Case::UnresolvedThread => {
            rows.legacy["data"]["node"]["isResolved"] = json!(false);
            rows.expanded["data"]["node"]["isResolved"] = json!(false);
            let mut threads = read_json(&state.join("threads.json"))?;
            threads["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"][0]["isResolved"] =
                json!(false);
            save_json(state, "threads.json", &threads)?;
        }
        _ => return Err("case is not a relationship mutation".into()),
    }
    Ok(())
}

fn mutate_change_request(state: &Path) -> Result<(), Box<dyn Error>> {
    save_json(
        state,
        "opinions.json",
        &json!({"data":{"repository":{"pullRequest":{"latestOpinionatedReviews":{
            "nodes":[{"id":"PRR_change_request","state":"CHANGES_REQUESTED",
                "submittedAt":"2026-10-27T14:40:00Z",
                "author":{"__typename":"User","databaseId":USER_ID}}],
            "pageInfo":{"hasNextPage":false,"endCursor":null}
        }}}}}),
    )
}

fn mutate_historical_commit(state: &Path) -> Result<(), Box<dyn Error>> {
    let mut pull = read_json(&state.join("pull.json"))?;
    pull["commits"] = json!(3);
    save_json(state, "pull.json", &pull)?;
    save_json(
        state,
        "commits.json",
        &json!([
            {"sha":HISTORICAL_COMMIT},
            {"sha":HISTORICAL_REPLY_COMMIT},
            {"sha":super::HEAD}
        ]),
    )
}

fn mutate_activity(state: &Path, case: Case) -> Result<(), Box<dyn Error>> {
    let (code, security) = if matches!(case, Case::LateActivity) {
        ("2026-10-27T15:20:00Z", "2026-10-27T15:05:00Z")
    } else {
        ("2026-10-27T15:20:00Z", "2026-10-27T15:25:00Z")
    };
    refresh_codex_terminals(state, code, security)
}

fn save_rows(state: &Path, mut rows: Rows) -> Result<(), Box<dyn Error>> {
    let editors = rows.editors.clone();
    rows.reviews
        .as_array_mut()
        .ok_or("reviews not array")?
        .extend(editors);
    save_json(state, "reviews.json", &rows.reviews)?;
    save_json(state, "review-comments.json", &rows.comments)?;
    save_json(state, "review-thread-full.json", &rows.legacy)?;
    save_json(state, "review-thread-human.json", &rows.expanded)?;
    for (index, review) in rows.editors.iter().enumerate() {
        let response = json!({"data":{"node":{
            "fullDatabaseId":review["id"].as_u64().ok_or("human review ID absent")?.to_string(),
            "body":review["body"],"submittedAt":review["submitted_at"],"lastEditedAt":null,
            "commit":{"oid":review["commit_id"]},
            "author":{"__typename":"User","databaseId":USER_ID,"login":"donbeave"},"editor":null
        }}});
        save_json(state, &format!("review-human-{index}.json"), &response)?;
    }
    Ok(())
}

fn refresh_codex_terminals(state: &Path, code: &str, security: &str) -> Result<(), Box<dyn Error>> {
    let mut comments = read_json(&state.join("issue-comments.json"))?;
    let summary = comments
        .as_array_mut()
        .ok_or("issue comments not an array")?
        .iter_mut()
        .find(|comment| {
            comment["body"]
                .as_str()
                .is_some_and(|body| body.starts_with("<!-- codex-pull-request-review-summary -->"))
        })
        .ok_or("Codex summary absent")?;
    let old_body = summary["body"].as_str().ok_or("Codex body absent")?;
    let new_body = old_body
        .replace("2026-10-27T14:58:52.571265Z", code)
        .replace("2026-10-27T15:03:24.469197Z", security);
    let updated = if code > security { code } else { security };
    summary["body"] = json!(new_body);
    summary["updated_at"] = json!(updated);
    save_json(state, "issue-comments.json", &comments)?;
    for name in ["codex-editor.json", "codex-full-editor.json"] {
        let mut editor = read_json(&state.join(name))?;
        editor["data"]["node"]["body"] = json!(new_body);
        editor["data"]["node"]["updatedAt"] = json!(updated);
        editor["data"]["node"]["lastEditedAt"] = json!(updated);
        editor["data"]["node"]["editor"] = json!({"__typename":"Bot","databaseId":199_175_422});
        save_json(state, name, &editor)?;
    }
    Ok(())
}

fn save_json(state: &Path, name: &str, value: &Value) -> Result<(), Box<dyn Error>> {
    fs::write(state.join(name), serde_json::to_vec(value)?)?;
    Ok(())
}
