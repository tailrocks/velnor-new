use super::{
    provider,
    state::{self, CompletionState, Operation, Proposal},
};
use crate::{OrchestratorError, internal::internal};
use serde_json::Value;
use std::path::Path;
use velnor_actions_contract::{canonical_json_bytes, digest_b3};

pub const CAPTURE_REVIEW_COMPLETION_OP: &str = "capture-review-completion-v1";

pub fn capture_review_completion() -> Result<Option<String>, OrchestratorError> {
    let operation = state::operation("review-completion")?;
    operation
        .deadline
        .scope(|| capture_until(&operation))
        .map_err(|_| internal("review_completion_deadline"))?
}

fn capture_until(operation: &Operation) -> Result<Option<String>, OrchestratorError> {
    let Some(number) = event_number(operation)? else {
        return Ok(None);
    };
    let proposal = observe(operation, number)?;
    let bytes =
        canonical_json_bytes(&proposal).map_err(|_| internal("review_completion_encode"))?;
    crate::exclusive_write::write_exclusive_until(
        &operation.temp.join(state::PROPOSAL_NAME),
        &bytes,
        "review_completion_proposal",
        || {
            operation
                .deadline
                .remaining()
                .map(|_| ())
                .map_err(|_| internal("review_completion_deadline"))
        },
    )?;
    Ok(Some(digest_b3(&bytes)))
}

pub(super) fn observe(operation: &Operation, number: u64) -> Result<Proposal, OrchestratorError> {
    let pull = state::pull(operation, number)?;
    let head = pull
        .pointer("/head/sha")
        .and_then(Value::as_str)
        .ok_or_else(|| internal("review_completion_head"))?;
    let mut proposal = Proposal {
        schema: "velnor.review-completion-proposal.v1".into(),
        policy_digest: digest_b3(
            &operation
                .policy
                .canonical_bytes()
                .map_err(|_| internal("review_completion_policy"))?,
        ),
        repository: operation.policy.repository.clone(),
        repository_id: operation.repository_id,
        pull_request_number: number,
        head_sha: head.into(),
        completion_app_id: operation.app_id,
        reviewer_app_id: state::PROVIDER_APP,
        feedback_digest: None,
        provider_digest: None,
        state: CompletionState::Blocked,
    };
    if let Ok(feedback) = state::feedback(operation, number, head) {
        proposal.feedback_digest = Some(
            feedback
                .snapshot
                .canonical_digest()
                .map_err(|_| internal("review_completion_feedback_digest"))?,
        );
        if let Ok((decision, digest)) = provider::inspect(
            &operation.api,
            head,
            &feedback.records,
            restart_hint(operation)?
                .as_ref()
                .map(|(role, stamp)| (*role, stamp.as_str())),
            (&operation.policy.repository, number),
        ) {
            proposal.provider_digest = digest;
            proposal.state = decision;
        }
        if feedback.snapshot.unresolved_threads != 0
            || feedback.snapshot.outstanding_change_requests != 0
        {
            proposal.state = CompletionState::Blocked;
        }
    }
    if pull["state"].as_str() != Some("open") || pull["draft"].as_bool() != Some(false) {
        proposal.state = CompletionState::Blocked;
    }
    let current = state::pull(operation, number)?;
    if current.pointer("/head/sha").and_then(Value::as_str) != Some(head)
        || current["state"] != pull["state"]
        || current["draft"] != pull["draft"]
    {
        return Err(internal("review_completion_pull_changed"));
    }
    Ok(proposal)
}

pub(super) fn event_number(operation: &Operation) -> Result<Option<u64>, OrchestratorError> {
    let bytes = state::read(
        Path::new(&state::env_string("GITHUB_EVENT_PATH")?),
        4 * 1024 * 1024,
        operation.deadline,
    )?;
    let event =
        velnor_actions_contract::strict_json::parse_strict_json_bytes(&bytes, 4 * 1024 * 1024)
            .map_err(|_| internal("review_completion_event"))?;
    if event.pointer("/repository/id").and_then(Value::as_u64) != Some(operation.repository_id)
        || event
            .pointer("/repository/full_name")
            .and_then(Value::as_str)
            != Some(operation.policy.repository.as_str())
    {
        return Err(internal("review_completion_event_repository"));
    }
    hint(&state::env_string("GITHUB_EVENT_NAME")?, &event)
}

fn hint(name: &str, event: &Value) -> Result<Option<u64>, OrchestratorError> {
    let number = match (name, event["action"].as_str()) {
        (
            "pull_request_target",
            Some("opened" | "synchronize" | "reopened" | "ready_for_review"),
        ) => event
            .pointer("/pull_request/number")
            .and_then(Value::as_u64),
        ("issue_comment", Some("created" | "edited" | "deleted"))
            if event
                .pointer("/issue/pull_request")
                .is_some_and(Value::is_object) =>
        {
            event.pointer("/issue/number").and_then(Value::as_u64)
        }
        ("issue_comment", Some("created" | "edited" | "deleted")) => return Ok(None),
        _ => return Err(internal("review_completion_event_kind")),
    };
    number
        .filter(|n| *n > 0)
        .map(Some)
        .ok_or_else(|| internal("review_completion_event_number"))
}

fn restart_hint(
    operation: &Operation,
) -> Result<Option<(&'static str, String)>, OrchestratorError> {
    if state::env_string("GITHUB_EVENT_NAME")? != "issue_comment" {
        return Ok(None);
    }
    let bytes = state::read(
        Path::new(&state::env_string("GITHUB_EVENT_PATH")?),
        4 * 1024 * 1024,
        operation.deadline,
    )?;
    let event =
        velnor_actions_contract::strict_json::parse_strict_json_bytes(&bytes, 4 * 1024 * 1024)
            .map_err(|_| internal("review_completion_event"))?;
    let role = match event.pointer("/comment/body").and_then(Value::as_str) {
        Some("@codex review") => "code",
        Some("@codex security review") => "security",
        _ => return Ok(None),
    };
    let stamp = event
        .pointer("/comment/updated_at")
        .and_then(Value::as_str)
        .ok_or_else(|| internal("review_completion_restart_time"))?;
    Ok(Some((role, stamp.to_owned())))
}
