//! Request-file materialization and response splitting for internal ops.

use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{
    FINAL_JSON_FILENAME, FinalStatus, MATRIX_JSON_FILENAME, NAMED_CHECK_LANES_ENV, NamedCheckLane,
    PLAN_JSON_FILENAME, QualificationDispatch, WorkflowEvent, canonical_json_bytes,
    matrix_json_bytes, plan_json_bytes, run_key_for_ci, validate_run_key,
};

use crate::OrchestratorError;
use crate::decisions::plan_artifact_dir;
use crate::internal::{
    MERGE_OP, PLAN_OP, PlanResponse, SCHEMA, check_schema, internal, internal_contract,
};
use crate::request_event::{
    QualificationRunnerContext, qualification_dispatch_for_parts, request_refs, workflow_event_for,
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
    /// Runner-owned repository slug (`owner/repo`) for provenance.
    ///
    /// Captured from `GITHUB_REPOSITORY` at the env boundary so the
    /// planner consumes an explicit capability instead of ambient env.
    /// Omitted when unset (local runs fall back to the git origin).
    #[serde(skip_serializing_if = "Option::is_none")]
    repository: Option<String>,
    /// Source-bound context for protected hosted qualification dispatches.
    #[serde(skip_serializing_if = "Option::is_none")]
    qualification: Option<QualificationDispatch>,
    /// Exact named-check job identities for lane expansion.
    #[serde(skip_serializing_if = "Option::is_none")]
    named_check_lanes: Option<BTreeMap<String, Vec<NamedCheckLane>>>,
}

#[path = "internal_request_plan.rs"]
mod plan;
pub(crate) use plan::{PlanRequest, checked_plan_request};

/// Materialize the canonical request file from the GitHub environment.
///
/// Reads the exact path from `VELNOR_REQUEST_FILE`, the event name from
/// `GITHUB_EVENT_NAME`, and the payload from `GITHUB_EVENT_PATH`, then
/// delegates to [`write_request_parts`]. The request file must sit under
/// `RUNNER_TEMP`: the runner owns that directory, so it anchors the
/// symlink-safe parent creation.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for missing env, malformed
/// payloads, anchor escapes, or unwritable paths;
/// [`OrchestratorError::Io`] for IO failures.
pub fn write_request() -> Result<PathBuf, OrchestratorError> {
    let path = env::var_os(crate::internal::REQUEST_FILE_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| internal("missing_request_file"))?;
    let anchor = env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| internal("missing_runner_temp"))?;
    let event_name = env::var("GITHUB_EVENT_NAME").unwrap_or_default();
    let payload_path = env::var_os("GITHUB_EVENT_PATH").filter(|value| !value.is_empty());
    let Some(payload_path) = payload_path else {
        return Err(internal("missing_event_payload"));
    };
    let payload_json = crate::safe_read::read_event_file(
        Path::new(&payload_path),
        crate::safe_read::MAX_REPO_FILE_BYTES,
    )?;
    let sha = env::var("GITHUB_SHA").ok().filter(|sha| !sha.is_empty());
    let repository = env::var(crate::origin::GITHUB_REPOSITORY_ENV)
        .ok()
        .filter(|slug| !slug.is_empty());
    let named_check_lanes = env::var(NAMED_CHECK_LANES_ENV)
        .ok()
        .map(|value| {
            serde_json::from_str(&value).map_err(|_| internal("malformed_named_check_lanes"))
        })
        .transpose()?;
    write_request_parts_with_lanes(
        &path,
        &event_name,
        &payload_json,
        sha.as_deref(),
        repository.as_deref(),
        &anchor,
        named_check_lanes,
    )
}

/// Materialize one canonical request file from explicit inputs.
///
/// The consuming op comes from the `<op>-request.json` file name; the file
/// is written exclusively (a pre-existing file errors, never overwritten).
/// The merge target assembles its request from downloaded artifacts and
/// ignores the event payload; the publish target records the push
/// refs plus the protected-branch evidence for the publish gate.
/// Parent directories are created under `anchor` with symlink refusal;
/// the caller passes the runner-owned directory the request path must
/// stay inside.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for unknown ops, unsupported
/// events, malformed payloads, anchor escapes, or unwritable paths.
pub fn write_request_parts(
    request_path: &Path,
    event_name: &str,
    payload_json: &str,
    github_sha: Option<&str>,
    repository: Option<&str>,
    anchor: &Path,
) -> Result<PathBuf, OrchestratorError> {
    write_request_parts_with_lanes(
        request_path,
        event_name,
        payload_json,
        github_sha,
        repository,
        anchor,
        None,
    )
}

fn write_request_parts_with_lanes(
    request_path: &Path,
    event_name: &str,
    payload_json: &str,
    github_sha: Option<&str>,
    repository: Option<&str>,
    anchor: &Path,
    named_check_lanes: Option<BTreeMap<String, Vec<NamedCheckLane>>>,
) -> Result<PathBuf, OrchestratorError> {
    let path = request_path.to_path_buf();
    let op = request_op(&path)?;
    if op == MERGE_OP {
        return crate::merge_request::write_merge_request(&path);
    }
    if op == crate::baseline_publish::PUBLISH_OP {
        return crate::baseline_publish::write_publish_request(
            &path,
            event_name,
            payload_json,
            github_sha,
            repository,
            anchor,
        );
    }
    let payload: serde_json::Value =
        serde_json::from_str(payload_json).map_err(|_| internal("malformed_event_payload"))?;
    let event = workflow_event_for(event_name, &payload)?;
    let qualification = qualification_dispatch_for_parts(
        event_name,
        &payload,
        QualificationRunnerContext {
            repository,
            git_ref: env::var("GITHUB_REF").ok().as_deref(),
            ref_protected: env::var("GITHUB_REF_PROTECTED").ok().as_deref(),
            workflow_ref: env::var("GITHUB_WORKFLOW_REF").ok().as_deref(),
            workflow_sha: env::var("GITHUB_WORKFLOW_SHA").ok().as_deref(),
            source_sha: github_sha,
            run_id: env::var("GITHUB_RUN_ID").ok().as_deref(),
            run_attempt: env::var("GITHUB_RUN_ATTEMPT").ok().as_deref(),
        },
    )?;
    let (base, head) = request_refs(event, &payload, github_sha)?;
    let request = EventRequest {
        schema: SCHEMA,
        op,
        event,
        base,
        head,
        root: ".".to_owned(),
        repository: repository
            .filter(|slug| !slug.is_empty())
            .map(str::to_owned),
        qualification,
        named_check_lanes,
    };
    let bytes = canonical_json_bytes(&request).map_err(internal_contract)?;
    if let Some(parent) = path.parent() {
        crate::exclusive_write::create_dir_no_symlink(anchor, parent)?;
    }
    crate::exclusive_write::write_exclusive(&path, &bytes, "request")?;
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
    let response = PlanResponse::parse(response_json)?;
    let dir = plan_artifact_dir(velnor_dir, &response.plan.run_key)?;
    write_plan_files(&response, velnor_dir, &dir)?;
    Ok(dir)
}

/// Write canonical `plan.json` plus `matrix.json` into one directory.
///
/// `matrix.json` is exactly `{"include": [...]}`: the same matrix the
/// plan step emits through `$GITHUB_OUTPUT`. When obligations covered,
/// the trusted `baseline.json` rides along so the merge revalidates
/// covered claims against the exact planner evidence.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for encoding failures and
/// [`OrchestratorError::Io`] for unwritable directories.
pub(crate) fn write_plan_files(
    response: &PlanResponse,
    velnor_dir: &Path,
    dir: &Path,
) -> Result<(), OrchestratorError> {
    response.validate()?;
    let plan = plan_json_bytes(&response.plan).map_err(internal_contract)?;
    let matrix = matrix_json_bytes(&response.plan.matrix).map_err(internal_contract)?;
    let baseline = response
        .baseline_manifest
        .as_ref()
        .map(canonical_json_bytes)
        .transpose()
        .map_err(internal_contract)?;
    crate::exclusive_write::create_dir_no_symlink(artifact_anchor(velnor_dir)?, dir)?;
    crate::exclusive_write::write_exclusive(&dir.join(PLAN_JSON_FILENAME), &plan, "plan_artifact")?;
    crate::exclusive_write::write_exclusive(
        &dir.join(MATRIX_JSON_FILENAME),
        &matrix,
        "plan_artifact",
    )?;
    if let Some(bytes) = baseline {
        crate::exclusive_write::write_exclusive(
            &dir.join(crate::baseline_publish::BASELINE_FILENAME),
            &bytes,
            "plan_artifact",
        )?;
    }
    Ok(())
}

/// Publish `final-report.json` for the final artifact.
///
/// Contract §3 fixes the verdict file under `<velnor-dir>/<run-key>/`
/// (the merge step passes `$RUNNER_TEMP/velnor`), which `Publish final
/// report` uploads; without it the upload fails with no-files-found.
/// Runs for every verdict, passing or not, before the exit-code check.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for malformed responses and
/// [`OrchestratorError::Io`] for unwritable directories.
pub fn publish_final_report(
    response_json: &str,
    velnor_dir: &Path,
) -> Result<PathBuf, OrchestratorError> {
    let report: velnor_actions_contract::FinalReport =
        serde_json::from_str(response_json).map_err(|_| internal("malformed_response"))?;
    check_schema(report.schema)?;
    let dir = plan_artifact_dir(velnor_dir, &report.run_key)?;
    crate::exclusive_write::create_dir_no_symlink(artifact_anchor(velnor_dir)?, &dir)?;
    let bytes = canonical_json_bytes(&report).map_err(internal_contract)?;
    crate::exclusive_write::write_exclusive(
        &dir.join(FINAL_JSON_FILENAME),
        &bytes,
        "plan_artifact",
    )?;
    Ok(dir)
}

/// True when one `merge-v1` response is a passing verdict.
///
/// Only `passed` passes: `no_work` proves nothing validated, so the gate
/// stays red; every other status fails.
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

/// Trust root for artifact dirs: the runner-owned parent of `velnor_dir`.
///
/// `velnor_dir` itself (`$RUNNER_TEMP/velnor`) is first created by a
/// producer, so it cannot anchor: a planted symlink there would sail
/// through its own anchor check. Its parent is runner-created (or
/// test-staged) and exists before any producer runs.
fn artifact_anchor(velnor_dir: &Path) -> Result<&Path, OrchestratorError> {
    velnor_dir
        .parent()
        .ok_or_else(|| internal("missing_dir_anchor"))
}

/// Consuming op from one `<op>-request.json` file name.
fn request_op(path: &Path) -> Result<String, OrchestratorError> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let op = name.strip_suffix("-request.json").unwrap_or_default();
    if op == PLAN_OP || op == MERGE_OP || op == crate::baseline_publish::PUBLISH_OP {
        Ok(op.to_owned())
    } else {
        Err(internal("unknown_request_op"))
    }
}
