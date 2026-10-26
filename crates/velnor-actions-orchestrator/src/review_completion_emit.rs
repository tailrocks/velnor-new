// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    capture,
    state::{self, CompletionState, Operation, Proposal},
};
use crate::{OrchestratorError, internal::internal};
use serde_json::Value;
use velnor_actions_contract::{
    canonical_json_bytes,
    homebrew_review_completion::{
        TAP_REVIEW_COMPLETION_SCHEMA, TapCompletedReviewer, TapCompletionCheckMetadata,
        TapReviewCompletionPayload, TapReviewVerdict, parse_tap_review_completion,
    },
};

pub const EMIT_REVIEW_COMPLETION_OP: &str = "emit-review-completion-v1";

pub fn emit_review_completion() -> Result<(), OrchestratorError> {
    let operation = state::operation("review-completion")?;
    operation
        .deadline
        .scope(|| emit_until(&operation))
        .map_err(|_| internal("review_completion_deadline"))?
}

fn emit_until(operation: &Operation) -> Result<(), OrchestratorError> {
    let proposed = state::read_proposal(
        operation,
        &state::env_string("VELNOR_REVIEW_COMPLETION_PROPOSAL_DIGEST")?,
    )?;
    if capture::event_number(operation)? != Some(proposed.pull_request_number) {
        return Err(internal("review_completion_proposal_pr"));
    }
    let pull = state::pull(operation, proposed.pull_request_number)?;
    let head = pull
        .pointer("/head/sha")
        .and_then(Value::as_str)
        .ok_or_else(|| internal("review_completion_head"))?;
    let id = unique_check(operation, head)?;
    let check = write(
        operation,
        id,
        head,
        false,
        "Review evidence is pending or blocked.",
    )?;
    let id = check["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("review_completion_check_id"))?;
    let fresh = capture::observe(operation, proposed.pull_request_number)?;
    if fresh != proposed || fresh.head_sha != head || fresh.state != CompletionState::Ready {
        return Ok(());
    }
    let text = completion(&fresh, &operation.policy.check_name)?;
    write(operation, Some(id), head, true, &text)?;
    match capture::observe(operation, proposed.pull_request_number) {
        Ok(after) if after == fresh => {}
        _ => {
            write(
                operation,
                Some(id),
                head,
                false,
                "Feedback changed during publication.",
            )?;
            return Err(internal("review_completion_changed_after_publication"));
        }
    }
    if unique_check(operation, head)? != Some(id) {
        return Err(internal("review_completion_check_changed"));
    }
    let current: Value = operation.api.get(&format!(
        "repos/{}/check-runs/{id}",
        operation.policy.repository
    ))?;
    validate_check(operation, &current, head, true)?;
    if current.pointer("/output/text").and_then(Value::as_str) != Some(text.as_str()) {
        return Err(internal("review_completion_check_output"));
    }
    Ok(())
}

fn unique_check(operation: &Operation, head: &str) -> Result<Option<u64>, OrchestratorError> {
    let runs =
        super::super::observation::check_runs(&operation.api, &operation.policy.repository, head)?;
    let matching = runs
        .iter()
        .filter(|run| {
            run["name"].as_str() == Some(operation.policy.check_name.as_str())
                && run.pointer("/app/id").and_then(Value::as_u64) == Some(operation.app_id)
        })
        .collect::<Vec<_>>();
    if matching.len() > 1 {
        return Err(internal("review_completion_check_ambiguous"));
    }
    matching
        .first()
        .map(|run| {
            if run["head_sha"].as_str() != Some(head) {
                return Err(internal("review_completion_check_head"));
            }
            run["id"]
                .as_u64()
                .filter(|id| *id > 0)
                .ok_or_else(|| internal("review_completion_check_id"))
        })
        .transpose()
}

fn write(
    operation: &Operation,
    id: Option<u64>,
    head: &str,
    ready: bool,
    text: &str,
) -> Result<Value, OrchestratorError> {
    let conclusion = if ready { "success" } else { "failure" };
    let mut fields = vec![
        ("name", operation.policy.check_name.clone()),
        ("status", "completed".into()),
        ("conclusion", conclusion.into()),
        ("output[title]", "Independent review completion".into()),
        (
            "output[summary]",
            if ready {
                "Authenticated exact-head review completed."
            } else {
                "Review is pending, changed, or blocked."
            }
            .into(),
        ),
        ("output[text]", text.to_owned()),
    ];
    if id.is_none() {
        fields.push(("head_sha", head.to_owned()));
    }
    let check = operation
        .api
        .write_completion_check(&operation.policy.repository, id, &fields)?;
    validate_check(operation, &check, head, ready)?;
    if id.is_some_and(|id| check["id"].as_u64() != Some(id)) {
        return Err(internal("review_completion_check_id"));
    }
    Ok(check)
}

fn validate_check(
    operation: &Operation,
    check: &Value,
    head: &str,
    ready: bool,
) -> Result<(), OrchestratorError> {
    if check.pointer("/app/id").and_then(Value::as_u64) != Some(operation.app_id)
        || check["name"].as_str() != Some(operation.policy.check_name.as_str())
        || check["head_sha"].as_str() != Some(head)
        || check["status"].as_str() != Some("completed")
        || check["conclusion"].as_str() != Some(if ready { "success" } else { "failure" })
    {
        return Err(internal("review_completion_check_binding"));
    }
    Ok(())
}

fn completion(proposal: &Proposal, name: &str) -> Result<String, OrchestratorError> {
    if proposal.state != CompletionState::Ready {
        return Err(internal("review_completion_not_ready"));
    }
    let payload = TapReviewCompletionPayload {
        schema: TAP_REVIEW_COMPLETION_SCHEMA.into(),
        repository: proposal.repository.clone(),
        pull_request_number: proposal.pull_request_number,
        head_sha: proposal.head_sha.clone(),
        verdict: TapReviewVerdict::Complete,
        feedback_digest: proposal
            .feedback_digest
            .clone()
            .ok_or_else(|| internal("review_completion_missing_feedback"))?,
        has_unaddressed_findings: false,
        reviewers: vec![TapCompletedReviewer {
            app_id: state::PROVIDER_APP,
            head_sha: proposal.head_sha.clone(),
            evidence_digest: proposal
                .provider_digest
                .clone()
                .ok_or_else(|| internal("review_completion_missing_provider"))?,
            complete: true,
            has_unaddressed_findings: false,
        }],
    };
    let bytes = canonical_json_bytes(&payload).map_err(|_| internal("review_completion_encode"))?;
    parse_tap_review_completion(
        &TapCompletionCheckMetadata {
            name: name.into(),
            app_id: proposal.completion_app_id,
            head_sha: proposal.head_sha.clone(),
            completed_successfully: true,
        },
        &bytes,
    )
    .map_err(|_| internal("review_completion_payload"))?;
    String::from_utf8(bytes).map_err(|_| internal("review_completion_encode"))
}
