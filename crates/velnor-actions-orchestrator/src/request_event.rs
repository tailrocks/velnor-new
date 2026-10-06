//! Runner-observed event resolution shared by plan and merge.

#[path = "request_scope.rs"]
pub(crate) mod request_scope;

use velnor_actions_contract::{VerificationScope, WorkflowEvent};

use crate::OrchestratorError;
use crate::internal::internal;
use crate::validators::validate_diff_rev;

/// Resolve one GitHub event name plus payload to a workflow event.
///
/// Shared by plan-request materialization and merge-time actual-event
/// capture so the two layers can never disagree about fork handling.
/// GitHub sends fork PRs as `pull_request` events (no distinct event
/// name); the head repo's `fork` flag is the only signal, and every
/// real `pull_request` payload carries it. A missing or non-boolean
/// flag fails closed (`indeterminate_fork`): defaulting to same-repo
/// would launder an untrusted fork run into the trusted PR scope.
/// Comment triggers (`issue_comment` and friends) have no mapping and
/// fail closed as `unsupported_event`: no comment path is trusted.
pub(crate) fn workflow_event_for(
    event_name: &str,
    payload: &serde_json::Value,
) -> Result<WorkflowEvent, OrchestratorError> {
    match event_name {
        "pull_request" => match fork_flag(payload) {
            Some(true) => Ok(WorkflowEvent::Fork),
            Some(false) => Ok(WorkflowEvent::PullRequest),
            None => Err(internal("indeterminate_fork")),
        },
        "push" => Ok(WorkflowEvent::Push),
        "merge_group" => Ok(WorkflowEvent::MergeGroup),
        "schedule" => Ok(WorkflowEvent::Schedule),
        "workflow_dispatch" => Ok(WorkflowEvent::WorkflowDispatch),
        "local" => Ok(WorkflowEvent::Local),
        _ => Err(internal("unsupported_event")),
    }
}

/// The head repo's `fork` flag, when present and boolean.
fn fork_flag(payload: &serde_json::Value) -> Option<bool> {
    payload["pull_request"]["head"]["repo"]["fork"].as_bool()
}

/// Base/head refs for one event: PR `base.sha`/`head.sha`, merge-group
/// `base_sha`/`head_sha`, push `before`/`after` (SHA fallback).
pub(crate) fn request_refs(
    event: WorkflowEvent,
    payload: &serde_json::Value,
    github_sha: Option<&str>,
) -> Result<(Option<String>, String), OrchestratorError> {
    request_refs_for_scope(event, payload, github_sha, VerificationScope::Affected)
}

/// Resolve refs while applying the dispatch scope's optional base input.
pub(crate) fn request_refs_for_scope(
    event: WorkflowEvent,
    payload: &serde_json::Value,
    github_sha: Option<&str>,
    scope: VerificationScope,
) -> Result<(Option<String>, String), OrchestratorError> {
    match event {
        WorkflowEvent::PullRequest => {
            let pr = &payload["pull_request"];
            Ok((
                Some(
                    nonempty(pr["base"]["sha"].as_str())
                        .ok_or_else(|| internal("missing_pr_base"))?,
                ),
                nonempty(pr["head"]["sha"].as_str()).ok_or_else(|| internal("missing_pr_head"))?,
            ))
        }
        WorkflowEvent::MergeGroup => {
            let group = &payload["merge_group"];
            Ok((
                Some(
                    nonempty(group["base_sha"].as_str())
                        .ok_or_else(|| internal("missing_merge_base"))?,
                ),
                nonempty(group["head_sha"].as_str())
                    .ok_or_else(|| internal("missing_merge_head"))?,
            ))
        }
        WorkflowEvent::Push => {
            let base = nonempty(payload["before"].as_str()).filter(|sha| !is_zero_sha(sha));
            let head = nonempty(payload["after"].as_str())
                .filter(|sha| !is_zero_sha(sha))
                .or_else(|| nonempty(github_sha))
                .ok_or_else(|| internal("missing_push_head"))?;
            Ok((base, head))
        }
        WorkflowEvent::Fork => {
            let pr = &payload["pull_request"];
            Ok((
                Some(
                    nonempty(pr["base"]["sha"].as_str())
                        .ok_or_else(|| internal("missing_pr_base"))?,
                ),
                nonempty(pr["head"]["sha"].as_str()).ok_or_else(|| internal("missing_pr_head"))?,
            ))
        }
        WorkflowEvent::Local => {
            let head = nonempty(github_sha).unwrap_or_else(|| "HEAD".to_owned());
            Ok((None, head))
        }
        WorkflowEvent::Schedule => Ok((
            None,
            nonempty(github_sha).ok_or_else(|| internal("missing_schedule_head"))?,
        )),
        WorkflowEvent::WorkflowDispatch => dispatch_refs(payload, github_sha, scope),
    }
}

/// Resolve the runner head and optional typed `workflow_dispatch` base input.
fn dispatch_refs(
    payload: &serde_json::Value,
    github_sha: Option<&str>,
    scope: VerificationScope,
) -> Result<(Option<String>, String), OrchestratorError> {
    let head = nonempty(github_sha).ok_or_else(|| internal("missing_dispatch_head"))?;
    let base = dispatch_base(payload)?;
    let base = if scope == VerificationScope::Full {
        None
    } else {
        base
    };
    Ok((base, head))
}

/// Parse and validate the optional dispatch base before scope can discard it.
fn dispatch_base(payload: &serde_json::Value) -> Result<Option<String>, OrchestratorError> {
    let Some(inputs) = payload.get("inputs") else {
        return Ok(None);
    };
    let Some(inputs) = inputs.as_object() else {
        return Err(internal("bad_base"));
    };
    let Some(value) = inputs.get("base_sha") else {
        return Ok(None);
    };
    let Some(raw) = value.as_str() else {
        return Err(internal("bad_base"));
    };
    let Some(base) = nonempty(Some(raw)) else {
        return Ok(None);
    };
    validate_diff_rev(&base, "bad_base").map_err(|problem| internal(&problem))?;
    Ok(Some(base))
}

/// Trimmed non-empty string, if any.
fn nonempty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// True for an all-zero (null) commit SHA.
fn is_zero_sha(sha: &str) -> bool {
    !sha.is_empty() && sha.bytes().all(|byte| byte == b'0')
}
