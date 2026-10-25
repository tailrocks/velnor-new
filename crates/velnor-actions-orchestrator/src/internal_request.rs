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

#[derive(Debug, Serialize)]
struct EventRequest {
    schema: u32,
    op: String,
    event: WorkflowEvent,
    base: Option<String>,
    head: String,
    root: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    repository: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    qualification: Option<QualificationDispatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    named_check_lanes: Option<BTreeMap<String, Vec<NamedCheckLane>>>,
}

#[path = "internal_request_plan.rs"]
mod plan;
pub(crate) use plan::{PlanRequest, checked_plan_request};

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

pub fn publish_plan_files(
    response_json: &str,
    velnor_dir: &Path,
) -> Result<PathBuf, OrchestratorError> {
    let response = PlanResponse::parse(response_json)?;
    let dir = plan_artifact_dir(velnor_dir, &response.plan.run_key)?;
    write_plan_files(&response, velnor_dir, &dir)?;
    Ok(dir)
}

pub(crate) fn write_plan_files(
    response: &PlanResponse,
    velnor_dir: &Path,
    dir: &Path,
) -> Result<(), OrchestratorError> {
    response.validate()?;
    let plan = plan_json_bytes(&response.plan).map_err(internal_contract)?;
    let matrix = matrix_json_bytes(&response.plan.matrix).map_err(internal_contract)?;
    crate::exclusive_write::create_dir_no_symlink(artifact_anchor(velnor_dir)?, dir)?;
    crate::exclusive_write::write_exclusive(&dir.join(PLAN_JSON_FILENAME), &plan, "plan_artifact")?;
    crate::exclusive_write::write_exclusive(
        &dir.join(MATRIX_JSON_FILENAME),
        &matrix,
        "plan_artifact",
    )?;
    Ok(())
}

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

use velnor_actions_workflow_renderer::render::PLAN_ATTEMPT_ENV;

pub(crate) fn resolve_run_key(explicit: Option<&str>) -> Result<String, OrchestratorError> {
    if let Some(key) = explicit.filter(|key| !key.trim().is_empty()) {
        validate_run_key(key).map_err(internal_contract)?;
        return Ok(key.to_owned());
    }
    let id = env::var("GITHUB_RUN_ID").ok().filter(|v| !v.is_empty());
    let attempt = env::var(PLAN_ATTEMPT_ENV)
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| {
            env::var("GITHUB_RUN_ATTEMPT")
                .ok()
                .filter(|v| !v.is_empty())
        });
    let (Some(id), Some(attempt)) = (id, attempt) else {
        return Err(internal("missing_run_key"));
    };
    let id: u64 = id.parse().map_err(|_| internal("bad_run_id"))?;
    let attempt: u64 = attempt.parse().map_err(|_| internal("bad_run_attempt"))?;
    Ok(run_key_for_ci(id, attempt))
}

fn artifact_anchor(velnor_dir: &Path) -> Result<&Path, OrchestratorError> {
    velnor_dir
        .parent()
        .ok_or_else(|| internal("missing_dir_anchor"))
}

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
