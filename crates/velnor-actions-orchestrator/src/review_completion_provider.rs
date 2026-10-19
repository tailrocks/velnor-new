//! Closed Codex-v1 adapter; human prose and reporting thresholds never prove readiness.
#[path = "review_completion_time.rs"]
mod time;
use super::{
    super::{feedback::FeedbackRecords, github::TapMergeGitHub},
    state::{CompletionState, PROVIDER_APP, PROVIDER_BOT, PROVIDER_OWNER},
};
use crate::{OrchestratorError, internal::internal};
use serde::Deserialize;
use serde_json::Value;
use velnor_actions_contract::{canonical_json_bytes, digest_b3};

#[path = "review_completion_findings.rs"]
mod findings;

const SUMMARY: &str = "<!-- codex-pull-request-review-summary -->";
const STATE: &str = "<!-- codex-review-state:v1 ";
const EDITOR: &str = "query($id:ID!){node(id:$id){... on IssueComment{databaseId body updatedAt lastEditedAt author{__typename ... on Bot{databaseId}} editor{__typename ... on Bot{databaseId}}}}}";
const REVIEW_EDITOR: &str = "query($id:ID!){node(id:$id){... on PullRequestReview{databaseId body submittedAt lastEditedAt commit{oid} author{__typename ... on Bot{databaseId}} editor{__typename ... on Bot{databaseId}}}}}";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewState {
    code_review: Role,
    security_review: Role,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Role {
    commit_sha: Option<String>,
    has_any_findings: Option<bool>,
    has_unresolved_findings: Option<bool>,
    status: Option<String>,
}

pub(super) fn inspect(
    api: &TapMergeGitHub,
    head: &str,
    records: &FeedbackRecords,
    restart: Option<(&str, &str)>,
    location: (&str, u64),
) -> Result<(CompletionState, Option<String>), OrchestratorError> {
    let Some(authenticated) = authenticate(api, records)? else {
        return Ok((CompletionState::Pending, None));
    };
    let (app, summary, editor) = authenticated;
    let body = summary["body"]
        .as_str()
        .ok_or_else(|| internal("review_provider_body"))?;
    let state = parse_state(body)?;
    let evidence = evidence_digest(&app, summary, &editor, &records.reviews, &[], restart)?;
    if !clean(&state.code_review, head)? || !clean(&state.security_review, head)? {
        return Ok((CompletionState::Pending, Some(evidence)));
    }
    if !records.commit_comments.is_empty() || !records.opinionated_reviews.is_empty() {
        return Ok((CompletionState::Blocked, Some(evidence)));
    }
    let code_time = time::terminal(body, "code")?;
    let security_time = time::terminal(body, "security")?;
    let finding = findings::authenticate(api, records, location)?;
    if finding
        .latest
        .is_some_and(|stamp| code_time <= stamp || security_time <= stamp)
    {
        return Ok((CompletionState::Pending, Some(evidence)));
    }
    let Ok(fresh) = requests(records, summary, code_time, security_time, restart) else {
        return Ok((CompletionState::Blocked, Some(evidence)));
    };
    if !fresh {
        return Ok((CompletionState::Pending, Some(evidence)));
    }
    let Some((covered, mut review_metadata)) =
        authenticate_reviews(api, records, (code_time, security_time))?
    else {
        return Ok((CompletionState::Blocked, Some(evidence)));
    };
    review_metadata.extend(finding.metadata);
    let evidence = evidence_digest(
        &app,
        summary,
        &editor,
        &records.reviews,
        &review_metadata,
        restart,
    )?;
    // The authenticated exact-head role summary owns completion. Clean reviews
    // emit no formal receipt; present receipts remain independent custody evidence.
    Ok((
        if covered {
            CompletionState::Ready
        } else {
            CompletionState::Pending
        },
        Some(evidence),
    ))
}

fn authenticate_reviews(
    api: &TapMergeGitHub,
    records: &FeedbackRecords,
    terminal: (time::Timestamp, time::Timestamp),
) -> Result<Option<(bool, Vec<Value>)>, OrchestratorError> {
    let mut covered = true;
    let mut ids = std::collections::BTreeSet::new();
    let mut review_metadata = Vec::new();
    for review in &records.reviews {
        let id = review["id"]
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or_else(|| internal("review_provider_review_id"))?;
        if !ids.insert(id)
            || review.pointer("/user/id").and_then(Value::as_u64) != Some(PROVIDER_BOT)
            || review["state"].as_str() != Some("COMMENTED")
            || !review["body"]
                .as_str()
                .is_some_and(|b| b.starts_with("<!-- codex-review-receipt:v1 -->"))
        {
            return Ok(None);
        }
        let code = receipt_is_code(
            review["body"]
                .as_str()
                .ok_or_else(|| internal("review_provider_receipt"))?,
        )?;
        let (metadata, activity) = authenticate_review(api, review)?;
        covered &= activity <= if code { terminal.0 } else { terminal.1 };
        review_metadata.push(metadata);
    }
    Ok(Some((covered, review_metadata)))
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

fn evidence_digest(
    app: &Value,
    summary: &Value,
    editor: &Value,
    reviews: &[Value],
    metadata: &[Value],
    restart: Option<(&str, &str)>,
) -> Result<String, OrchestratorError> {
    Ok(digest_b3(
        &canonical_json_bytes(&(app, summary, editor, reviews, metadata, restart))
            .map_err(|_| internal("review_provider_encode"))?,
    ))
}

type AuthenticatedSummary<'a> = (Value, &'a Value, Value);

fn authenticate<'a>(
    api: &TapMergeGitHub,
    records: &'a FeedbackRecords,
) -> Result<Option<AuthenticatedSummary<'a>>, OrchestratorError> {
    let app: Value = api.get("apps/chatgpt-codex-connector")?;
    if app["id"].as_u64() != Some(PROVIDER_APP)
        || app["slug"].as_str() != Some("chatgpt-codex-connector")
        || app.pointer("/owner/id").and_then(Value::as_u64) != Some(PROVIDER_OWNER)
    {
        return Err(internal("review_provider_app"));
    }
    let summaries = records
        .issue_comments
        .iter()
        .filter(|c| c["body"].as_str().is_some_and(|b| b.starts_with(SUMMARY)))
        .collect::<Vec<_>>();
    if summaries.len() != 1 {
        return Ok(None);
    }
    let summary = summaries[0];
    authenticate_summary(summary)?;
    let node_id = summary["node_id"]
        .as_str()
        .ok_or_else(|| internal("review_provider_node"))?;
    let editor: Value = api.graphql(EDITOR, &[("id", node_id.to_owned())])?;
    validate_editor(summary, &editor)?;
    Ok(Some((app, summary, editor)))
}

fn requests(
    records: &FeedbackRecords,
    summary: &Value,
    code_time: time::Timestamp,
    security_time: time::Timestamp,
    restart: Option<(&str, &str)>,
) -> Result<bool, OrchestratorError> {
    if let Some((role, stamp)) = restart {
        let stamp = time::parse(stamp)?;
        if (role == "code" && code_time <= stamp) || (role == "security" && security_time <= stamp)
        {
            return Ok(false);
        }
    }
    let mut ids = std::collections::BTreeSet::new();
    for comment in &records.issue_comments {
        let id = comment["id"]
            .as_u64()
            .filter(|id| *id > 0)
            .ok_or_else(|| internal("review_provider_comment_id"))?;
        if !ids.insert(id) {
            return Err(internal("review_provider_comment_id"));
        }
        if comment["id"] == summary["id"] {
            continue;
        }
        let Some(text) = comment["body"].as_str() else {
            return Err(internal("review_provider_unknown_feedback"));
        };
        let role = match text {
            "@codex review" => "code",
            "@codex security review" => "security",
            _ => return Err(internal("review_provider_unknown_feedback")),
        };
        let stamp = time::parse(
            comment["updated_at"]
                .as_str()
                .ok_or_else(|| internal("review_request_time"))?,
        )?;
        if (role == "code" && code_time <= stamp) || (role == "security" && security_time <= stamp)
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn authenticate_review(
    api: &TapMergeGitHub,
    review: &Value,
) -> Result<(Value, time::Timestamp), OrchestratorError> {
    let submitted = time::parse(
        review["submitted_at"]
            .as_str()
            .ok_or_else(|| internal("review_provider_review_time"))?,
    )?;
    let id = review["node_id"]
        .as_str()
        .ok_or_else(|| internal("review_provider_review_node"))?;
    let response: Value = api.graphql(REVIEW_EDITOR, &[("id", id.to_owned())])?;
    let node = &response["data"]["node"];
    if response.get("errors").is_some()
        || node["databaseId"] != review["id"]
        || node["body"] != review["body"]
        || node["submittedAt"] != review["submitted_at"]
        || node.pointer("/commit/oid").and_then(Value::as_str) != review["commit_id"].as_str()
        || node.pointer("/author/databaseId").and_then(Value::as_u64) != Some(PROVIDER_BOT)
        || node.pointer("/author/__typename").and_then(Value::as_str) != Some("Bot")
    {
        return Err(internal("review_provider_review_editor"));
    }
    if node.get("lastEditedAt").is_none() || node.get("editor").is_none() {
        return Err(internal("review_provider_review_editor"));
    }
    let mut activity = submitted;
    if !node["lastEditedAt"].is_null() || !node["editor"].is_null() {
        activity = time::parse(
            node["lastEditedAt"]
                .as_str()
                .ok_or_else(|| internal("review_provider_review_editor"))?,
        )?;
        if activity < submitted
            || node.pointer("/editor/databaseId").and_then(Value::as_u64) != Some(PROVIDER_BOT)
            || node.pointer("/editor/__typename").and_then(Value::as_str) != Some("Bot")
        {
            return Err(internal("review_provider_review_editor"));
        }
    }
    Ok((response, activity))
}

fn authenticate_summary(summary: &Value) -> Result<(), OrchestratorError> {
    let created = time::parse(
        summary["created_at"]
            .as_str()
            .ok_or_else(|| internal("review_provider_time"))?,
    )?;
    let updated = time::parse(
        summary["updated_at"]
            .as_str()
            .ok_or_else(|| internal("review_provider_time"))?,
    )?;
    if updated < created {
        return Err(internal("review_provider_time"));
    }
    if summary["id"].as_u64().is_none_or(|id| id == 0)
        || summary.pointer("/user/id").and_then(Value::as_u64) != Some(PROVIDER_BOT)
        || summary.pointer("/user/type").and_then(Value::as_str) != Some("Bot")
        || summary
            .pointer("/performed_via_github_app/id")
            .and_then(Value::as_u64)
            != Some(PROVIDER_APP)
        || summary
            .pointer("/performed_via_github_app/owner/id")
            .and_then(Value::as_u64)
            != Some(PROVIDER_OWNER)
    {
        return Err(internal("review_provider_author"));
    }
    Ok(())
}

fn validate_editor(summary: &Value, response: &Value) -> Result<(), OrchestratorError> {
    let node = &response["data"]["node"];
    if node.get("lastEditedAt").is_none() || node.get("editor").is_none() {
        return Err(internal("review_provider_editor"));
    }
    if response.get("errors").is_some()
        || node["databaseId"] != summary["id"]
        || node["body"] != summary["body"]
        || node["updatedAt"] != summary["updated_at"]
        || node.pointer("/author/databaseId").and_then(Value::as_u64) != Some(PROVIDER_BOT)
        || node.pointer("/author/__typename").and_then(Value::as_str) != Some("Bot")
    {
        return Err(internal("review_provider_editor"));
    }
    if node["lastEditedAt"].is_null() && node["editor"].is_null() {
        if summary["created_at"] != summary["updated_at"] {
            return Err(internal("review_provider_editor"));
        }
        return Ok(());
    }
    let edited = time::parse(
        node["lastEditedAt"]
            .as_str()
            .ok_or_else(|| internal("review_provider_editor"))?,
    )?;
    let updated = time::parse(
        summary["updated_at"]
            .as_str()
            .ok_or_else(|| internal("review_provider_editor"))?,
    )?;
    if edited != updated {
        return Err(internal("review_provider_editor"));
    }
    if node.pointer("/editor/databaseId").and_then(Value::as_u64) != Some(PROVIDER_BOT)
        || node.pointer("/editor/__typename").and_then(Value::as_str) != Some("Bot")
    {
        return Err(internal("review_provider_editor"));
    }
    Ok(())
}

fn parse_state(body: &str) -> Result<ReviewState, OrchestratorError> {
    let rows = body
        .lines()
        .filter_map(|line| line.strip_prefix(STATE))
        .collect::<Vec<_>>();
    if rows.len() != 1 {
        return Err(internal("review_provider_state"));
    }
    let json = rows[0]
        .strip_suffix(" -->")
        .ok_or_else(|| internal("review_provider_state"))?;
    let value =
        velnor_actions_contract::strict_json::parse_strict_json_bytes(json.as_bytes(), 64 * 1024)
            .map_err(|_| internal("review_provider_state"))?;
    serde_json::from_value(value).map_err(|_| internal("review_provider_state"))
}
fn clean(role: &Role, head: &str) -> Result<bool, OrchestratorError> {
    if !matches!(
        role.status.as_deref(),
        None | Some("queued" | "running" | "completed" | "failed" | "cancelled" | "timed_out")
    ) {
        return Err(internal("review_provider_status"));
    }
    Ok(role.commit_sha.as_deref() == Some(head)
        && role.status.as_deref() == Some("completed")
        && role.has_any_findings == Some(false)
        && role.has_unresolved_findings == Some(false))
}

#[cfg(test)]
#[path = "review_completion_provider_tests.rs"]
mod tests;
