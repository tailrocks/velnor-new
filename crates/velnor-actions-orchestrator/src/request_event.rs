//! Runner-observed event resolution shared by plan and merge.

use velnor_actions_contract::workflow::QUALIFICATION_WORKFLOW_PATH;
use velnor_actions_contract::{
    QualificationDispatch, QualificationDispatchMode, QualificationPhase, QualificationRunRef,
    WorkflowEvent, verification_repeat_campaign,
};

use crate::OrchestratorError;
use crate::internal::internal;

/// Runner-owned GitHub context bound into a qualification plan.
#[derive(Debug, Clone, Copy)]
pub(crate) struct QualificationRunnerContext<'a> {
    pub(crate) repository: Option<&'a str>,
    pub(crate) git_ref: Option<&'a str>,
    pub(crate) ref_protected: Option<&'a str>,
    pub(crate) workflow_ref: Option<&'a str>,
    pub(crate) workflow_sha: Option<&'a str>,
    pub(crate) source_sha: Option<&'a str>,
    pub(crate) run_id: Option<&'a str>,
    pub(crate) run_attempt: Option<&'a str>,
}

struct QualificationDispatchSource {
    repository: String,
    default_branch: String,
    git_ref: String,
    workflow_ref: String,
    workflow_sha: String,
    source_sha: String,
}

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
        "workflow_dispatch" => Ok(WorkflowEvent::Qualification),
        "local" => Ok(WorkflowEvent::Local),
        _ => Err(internal("unsupported_event")),
    }
}

/// Build a typed qualification descriptor from the actual dispatch payload
/// and runner-owned GitHub context.
pub(crate) fn qualification_dispatch_for_parts(
    event_name: &str,
    payload: &serde_json::Value,
    runner: QualificationRunnerContext<'_>,
) -> Result<Option<QualificationDispatch>, OrchestratorError> {
    if event_name != "workflow_dispatch" {
        return Ok(None);
    }
    let inputs = &payload["inputs"];
    let payload_repository = nonempty(payload["repository"]["full_name"].as_str())
        .ok_or_else(|| internal("missing_qualification_payload_repository"))?;
    let default_branch = nonempty(payload["repository"]["default_branch"].as_str())
        .ok_or_else(|| internal("missing_qualification_default_branch"))?;
    let payload_ref = nonempty(payload["ref"].as_str())
        .ok_or_else(|| internal("missing_qualification_payload_ref"))?;
    let repository = required_context(runner.repository, "repository")?;
    let git_ref = required_context(runner.git_ref, "ref")?;
    if payload_repository != repository || payload_ref != git_ref {
        return Err(internal("qualification_payload_context_mismatch"));
    }
    let source = QualificationDispatchSource {
        repository,
        default_branch,
        git_ref,
        workflow_ref: required_context(runner.workflow_ref, "workflow_ref")?,
        workflow_sha: required_context(runner.workflow_sha, "workflow_sha")?,
        source_sha: required_context(runner.source_sha, "source_sha")?,
    };
    let mode = qualification_mode(inputs)?;
    let run_id = runner
        .run_id
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| internal("invalid_qualification_run_id"))?;
    let run_attempt = runner
        .run_attempt
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| internal("invalid_qualification_run_attempt"))?;
    let (campaign, phase, predecessor) =
        qualification_selection(inputs, mode, &source, run_id, run_attempt)?;
    let QualificationDispatchSource {
        repository,
        default_branch,
        git_ref,
        workflow_ref,
        workflow_sha,
        source_sha,
    } = source;
    let context = QualificationDispatch {
        mode,
        campaign,
        phase,
        repository,
        default_branch,
        git_ref,
        ref_protected: runner.ref_protected == Some("true"),
        workflow_ref,
        workflow_sha,
        source_sha,
        run_id,
        run_attempt,
        predecessor,
        scale_set_image_source_sha: None,
    };
    context
        .validate_shape()
        .map_err(|_| internal("invalid_qualification_context"))?;
    Ok(Some(context))
}

fn qualification_selection(
    inputs: &serde_json::Value,
    mode: QualificationDispatchMode,
    source: &QualificationDispatchSource,
    run_id: u64,
    run_attempt: u32,
) -> Result<(String, QualificationPhase, Option<QualificationRunRef>), OrchestratorError> {
    match mode {
        QualificationDispatchMode::Controlled => {
            if source.git_ref != format!("refs/heads/{}", source.default_branch)
                && is_qualification_wrapper(
                    &source.workflow_ref,
                    &source.repository,
                    &source.git_ref,
                )
            {
                return Err(internal("qualification_wrapper_requires_verification_only"));
            }
            let campaign = nonempty(inputs["campaign"].as_str())
                .ok_or_else(|| internal("missing_qualification_campaign"))?;
            let phase = match inputs["phase"].as_str() {
                Some("cold") => QualificationPhase::Cold,
                Some("warm") => QualificationPhase::Warm,
                Some("third") => QualificationPhase::Third,
                Some("useful_delta") => QualificationPhase::UsefulDelta,
                Some("control") => QualificationPhase::Control,
                _ => return Err(internal("invalid_qualification_phase")),
            };
            let predecessor = qualification_predecessor(inputs)?;
            Ok((campaign, phase, predecessor))
        }
        QualificationDispatchMode::VerificationOnly => {
            if !is_qualification_wrapper(&source.workflow_ref, &source.repository, &source.git_ref)
            {
                return Err(internal("verification_only_requires_qualification_wrapper"));
            }
            if !has_only_mode_input(inputs) {
                return Err(internal("verification_only_rejects_extra_inputs"));
            }
            let campaign = verification_repeat_campaign(
                &source.repository,
                &source.default_branch,
                &source.git_ref,
                &source.workflow_ref,
                &source.workflow_sha,
                &source.source_sha,
                run_id,
            );
            let phase = if run_attempt == 1 {
                QualificationPhase::Cold
            } else {
                QualificationPhase::Warm
            };
            let predecessor = (run_attempt > 1).then_some(QualificationRunRef {
                run_id,
                run_attempt: 1,
            });
            Ok((campaign, phase, predecessor))
        }
    }
}

fn qualification_mode(
    inputs: &serde_json::Value,
) -> Result<QualificationDispatchMode, OrchestratorError> {
    match inputs.as_object().and_then(|values| values.get("mode")) {
        None => Ok(QualificationDispatchMode::Controlled),
        Some(value) if value.as_str() == Some("both") => Ok(QualificationDispatchMode::Controlled),
        Some(value) if value.as_str() == Some("verification-only") => {
            Ok(QualificationDispatchMode::VerificationOnly)
        }
        Some(_) => Err(internal("invalid_qualification_mode")),
    }
}

fn has_only_mode_input(inputs: &serde_json::Value) -> bool {
    inputs
        .as_object()
        .is_some_and(|values| values.len() == 1 && values.contains_key("mode"))
}

fn is_qualification_wrapper(workflow_ref: &str, repository: &str, git_ref: &str) -> bool {
    workflow_ref == format!("{repository}/{QUALIFICATION_WORKFLOW_PATH}@{git_ref}")
}

/// Parse the optional paired run locator. The contract applies phase rules;
/// this boundary rejects malformed or partial identifiers before admission.
fn qualification_predecessor(
    inputs: &serde_json::Value,
) -> Result<Option<QualificationRunRef>, OrchestratorError> {
    let run_id = nonempty(inputs["predecessor_run_id"].as_str());
    let run_attempt = nonempty(inputs["predecessor_run_attempt"].as_str());
    match (run_id, run_attempt) {
        (None, None) => Ok(None),
        (Some(run_id), Some(run_attempt)) => {
            let run_id = run_id
                .parse::<u64>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(|| internal("invalid_qualification_predecessor_run_id"))?;
            let run_attempt = run_attempt
                .parse::<u32>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(|| internal("invalid_qualification_predecessor_run_attempt"))?;
            Ok(Some(QualificationRunRef {
                run_id,
                run_attempt,
            }))
        }
        _ => Err(internal("incomplete_qualification_predecessor")),
    }
}

/// Require one non-empty runner-owned context value.
fn required_context(value: Option<&str>, name: &str) -> Result<String, OrchestratorError> {
    nonempty(value).ok_or_else(|| internal(&format!("missing_qualification_{name}")))
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
        WorkflowEvent::Qualification => Ok((
            None,
            nonempty(github_sha).ok_or_else(|| internal("missing_qualification_head"))?,
        )),
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
    }
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

#[cfg(test)]
#[path = "request_event_tests.rs"]
mod tests;
