//! Request-file materialization and response splitting for internal ops.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{
    FinalStatus, WorkflowEvent, canonical_json_bytes, canonical_json_str, run_key_for_ci,
    validate_run_key,
};

use crate::OrchestratorError;
use crate::decisions::plan_artifact_dir;
use crate::internal::{
    MERGE_OP, PLAN_OP, PlanResponse, SCHEMA, check_schema, internal, internal_contract,
};

/// Plan-time request: `{schema, op, event, base, head, root}` (schema 1).
#[derive(Debug, Serialize)]
struct EventRequest {
    /// Request schema; always 1.
    schema: u32,
    /// Consuming operation (`plan-v1` or `merge-v1`).
    op: String,
    /// Triggering event.
    event: WorkflowEvent,
    /// Base commit or null.
    base: Option<String>,
    /// Head commit.
    head: String,
    /// Repository root; always `.` (the job checkout).
    root: String,
}

/// Canonical `plan`/`matrix` outputs for `$GITHUB_OUTPUT`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanOutputs {
    /// Canonical plan JSON (single line).
    pub plan: String,
    /// Canonical matrix JSON (single line).
    pub matrix: String,
}

/// Materialize the canonical request file from the GitHub environment.
///
/// Reads the exact path from `VELNOR_REQUEST_FILE`, the event name from
/// `GITHUB_EVENT_NAME`, and the payload from `GITHUB_EVENT_PATH`, then
/// delegates to [`write_request_parts`].
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for missing env, malformed
/// payloads, or unwritable paths; [`OrchestratorError::Io`] for IO failures.
pub fn write_request() -> Result<PathBuf, OrchestratorError> {
    let path = env::var_os(crate::internal::REQUEST_FILE_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| internal("missing_request_file"))?;
    let event_name = env::var("GITHUB_EVENT_NAME").unwrap_or_default();
    let payload_path = env::var_os("GITHUB_EVENT_PATH").filter(|value| !value.is_empty());
    let Some(payload_path) = payload_path else {
        return Err(internal("missing_event_payload"));
    };
    let payload_json =
        fs::read_to_string(&payload_path).map_err(|_| internal("unreadable_event_payload"))?;
    let sha = env::var("GITHUB_SHA").ok().filter(|sha| !sha.is_empty());
    write_request_parts(&path, &event_name, &payload_json, sha.as_deref())
}

/// Materialize one canonical request file from explicit inputs.
///
/// The consuming op comes from the `<op>-request.json` file name; the file
/// is written exclusively (a pre-existing file errors, never overwritten).
/// The merge target assembles its request from downloaded artifacts and
/// ignores the event payload.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for unknown ops, unsupported
/// events, malformed payloads, or unwritable paths.
pub fn write_request_parts(
    request_path: &Path,
    event_name: &str,
    payload_json: &str,
    github_sha: Option<&str>,
) -> Result<PathBuf, OrchestratorError> {
    let path = request_path.to_path_buf();
    let op = request_op(&path)?;
    if op == MERGE_OP {
        return crate::merge_request::write_merge_request(&path);
    }
    let event = match event_name {
        "pull_request" => WorkflowEvent::PullRequest,
        "push" => WorkflowEvent::Push,
        "merge_group" => WorkflowEvent::MergeGroup,
        _ => return Err(internal("unsupported_event")),
    };
    let payload: serde_json::Value =
        serde_json::from_str(payload_json).map_err(|_| internal("malformed_event_payload"))?;
    let (base, head) = request_refs(event, &payload, github_sha)?;
    let request = EventRequest {
        schema: SCHEMA,
        op,
        event,
        base,
        head,
        root: ".".to_owned(),
    };
    let bytes = canonical_json_bytes(&request).map_err(internal_contract)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| OrchestratorError::io(parent.display().to_string(), err.to_string()))?;
    }
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| internal("request_exists"))
        .and_then(|mut file| {
            use std::io::Write;
            file.write_all(&bytes)
                .map_err(|_| internal("request_unwritable"))
        })?;
    Ok(path)
}

/// Derive the sibling `<op>-response.json` path for one request path.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] unless the file name is exactly
/// `<op>-request.json` with a non-empty op.
pub fn response_path_for(request_path: &Path) -> Result<PathBuf, OrchestratorError> {
    let name = request_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let Some(op) = name
        .strip_suffix("-request.json")
        .filter(|op| !op.is_empty())
    else {
        return Err(internal("bad_request_file_name"));
    };
    let parent = request_path.parent().unwrap_or_else(|| Path::new("."));
    Ok(parent.join(format!("{op}-response.json")))
}

/// Split one `plan-v1` response into canonical `$GITHUB_OUTPUT` values.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for malformed responses.
pub fn plan_outputs(response_json: &str) -> Result<PlanOutputs, OrchestratorError> {
    let response: PlanResponse =
        serde_json::from_str(response_json).map_err(|_| internal("malformed_response"))?;
    check_schema(response.schema)?;
    Ok(PlanOutputs {
        plan: canonical_json_str(&response.plan).map_err(internal_contract)?,
        matrix: canonical_json_str(&response.matrix).map_err(internal_contract)?,
    })
}

/// Publish `plan.json` + `matrix.json` for the plan artifact.
///
/// Contract §4 fixes the artifact content under `<velnor-dir>/<run-key>/`
/// (the plan step passes `$RUNNER_TEMP/velnor`), which `Publish plan`
/// uploads; without these files the upload fails with no-files-found.
/// The run key comes from the response plan, so the files always land in
/// the directory the upload step names.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for malformed responses and
/// [`OrchestratorError::Io`] for unwritable directories.
pub fn publish_plan_files(
    response_json: &str,
    velnor_dir: &Path,
) -> Result<PathBuf, OrchestratorError> {
    let response: PlanResponse =
        serde_json::from_str(response_json).map_err(|_| internal("malformed_response"))?;
    check_schema(response.schema)?;
    let dir = plan_artifact_dir(velnor_dir, &response.plan.run_key)?;
    write_plan_files(&response, &dir)?;
    Ok(dir)
}

/// Write canonical `plan.json` plus `matrix.json` into one directory.
///
/// `matrix.json` is exactly `{"include": [...]}`: the same matrix the
/// plan step emits through `$GITHUB_OUTPUT`.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for encoding failures and
/// [`OrchestratorError::Io`] for unwritable directories.
pub(crate) fn write_plan_files(
    response: &PlanResponse,
    dir: &Path,
) -> Result<(), OrchestratorError> {
    fs::create_dir_all(dir)
        .map_err(|err| OrchestratorError::io(dir.display().to_string(), err.to_string()))?;
    let plan = canonical_json_bytes(&response.plan).map_err(internal_contract)?;
    let matrix = canonical_json_bytes(&response.matrix).map_err(internal_contract)?;
    write_new(&dir.join("plan.json"), &plan)?;
    write_new(&dir.join("matrix.json"), &matrix)?;
    Ok(())
}

/// Exclusively write one artifact file; a pre-existing file errors.
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), OrchestratorError> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| internal("plan_artifact_exists"))
        .and_then(|mut file| {
            use std::io::Write;
            file.write_all(bytes)
                .map_err(|_| internal("plan_artifact_unwritable"))
        })
}

/// True when one `merge-v1` response is a passing verdict.
///
/// `passed` and `no_work` pass; every other status fails.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for malformed responses.
pub fn merge_passed(response_json: &str) -> Result<bool, OrchestratorError> {
    #[derive(Debug, Deserialize)]
    struct Verdict {
        status: FinalStatus,
    }
    let verdict: Verdict =
        serde_json::from_str(response_json).map_err(|_| internal("malformed_response"))?;
    Ok(matches!(
        verdict.status,
        FinalStatus::Passed | FinalStatus::NoWork
    ))
}

/// Explicit run key, else `r<run-id>-a<attempt>` from the GitHub environment.
pub(crate) fn resolve_run_key(explicit: Option<&str>) -> Result<String, OrchestratorError> {
    if let Some(key) = explicit.filter(|key| !key.trim().is_empty()) {
        validate_run_key(key).map_err(internal_contract)?;
        return Ok(key.to_owned());
    }
    let id = env::var("GITHUB_RUN_ID").ok().filter(|v| !v.is_empty());
    let attempt = env::var("GITHUB_RUN_ATTEMPT")
        .ok()
        .filter(|v| !v.is_empty());
    let (Some(id), Some(attempt)) = (id, attempt) else {
        return Err(internal("missing_run_key"));
    };
    let id: u64 = id.parse().map_err(|_| internal("bad_run_id"))?;
    let attempt: u64 = attempt.parse().map_err(|_| internal("bad_run_attempt"))?;
    Ok(run_key_for_ci(id, attempt))
}

/// Consuming op from one `<op>-request.json` file name.
fn request_op(path: &Path) -> Result<String, OrchestratorError> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let op = name.strip_suffix("-request.json").unwrap_or_default();
    if op == PLAN_OP || op == MERGE_OP {
        Ok(op.to_owned())
    } else {
        Err(internal("unknown_request_op"))
    }
}

/// Base/head refs for one event: PR `base.sha`/`head.sha`, merge-group
/// `base_sha`/`head_sha`, push `before`/`after` (SHA fallback).
fn request_refs(
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
