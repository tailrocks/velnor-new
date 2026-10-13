//! Request-file materialization and response splitting for internal ops.

use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};

use serde::Serialize;
use velnor_actions_contract::canonical_json_bytes;
use velnor_actions_contract_config::ExecutionMode;
use velnor_actions_contract_workflow::{
    EXECUTION_MODE_ENV, NAMED_CHECK_LANES_ENV, NamedCheckLane, WorkflowEvent,
};

use crate::internal::{MERGE_OP, PLAN_OP, SCHEMA};
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::{internal, internal_contract};
use velnor_actions_orchestrator_request_event::request_event::{request_refs, workflow_event_for};

mod outputs;
pub use outputs::{
    PlanOutputs, merge_passed, plan_outputs, publish_final_report, publish_plan_files,
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
    /// Exact emitted named-check job and report identities, when schema 2
    /// expands checks across execution lanes.
    #[serde(skip_serializing_if = "Option::is_none")]
    named_check_lanes: Option<BTreeMap<String, Vec<NamedCheckLane>>>,
    /// Effective schema-2 provider mode selected while rendering this workflow.
    #[serde(skip_serializing_if = "Option::is_none")]
    execution_mode: Option<ExecutionMode>,
}

/// Optional routing values passed from the rendered workflow to the planner.
#[derive(Debug, Default)]
struct LaneSelection {
    named_check_lanes: Option<BTreeMap<String, Vec<NamedCheckLane>>>,
    execution_mode: Option<ExecutionMode>,
}

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
    let payload_json = velnor_actions_orchestrator_core::safe_read::read_event_file(
        Path::new(&payload_path),
        velnor_actions_orchestrator_core::safe_read::MAX_REPO_FILE_BYTES,
    )?;
    let sha = env::var("GITHUB_SHA").ok().filter(|sha| !sha.is_empty());
    let repository = env::var(velnor_actions_orchestrator_core::origin::GITHUB_REPOSITORY_ENV)
        .ok()
        .filter(|slug| !slug.is_empty());
    let named_check_lanes = env::var(NAMED_CHECK_LANES_ENV)
        .ok()
        .map(|value| {
            serde_json::from_str(&value).map_err(|_| internal("malformed_named_check_lanes"))
        })
        .transpose()?;
    let execution_mode = env::var(EXECUTION_MODE_ENV)
        .ok()
        .map(|value| ExecutionMode::parse(&value).map_err(internal_contract))
        .transpose()?;
    write_request_parts_with_lanes(
        &path,
        &event_name,
        &payload_json,
        sha.as_deref(),
        repository.as_deref(),
        &anchor,
        LaneSelection {
            named_check_lanes,
            execution_mode,
        },
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
        LaneSelection::default(),
    )
}

fn write_request_parts_with_lanes(
    request_path: &Path,
    event_name: &str,
    payload_json: &str,
    github_sha: Option<&str>,
    repository: Option<&str>,
    anchor: &Path,
    lane_selection: LaneSelection,
) -> Result<PathBuf, OrchestratorError> {
    let path = request_path.to_path_buf();
    let op = request_op(&path)?;
    if op == MERGE_OP {
        return crate::merge_request::write_merge_request(&path);
    }
    if op == velnor_actions_orchestrator_baseline_publish::baseline_publish::PUBLISH_OP {
        return velnor_actions_orchestrator_baseline_publish::baseline_publish::write_publish_request(
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
        named_check_lanes: lane_selection.named_check_lanes,
        execution_mode: lane_selection.execution_mode,
    };
    let bytes = canonical_json_bytes(&request).map_err(internal_contract)?;
    if let Some(parent) = path.parent() {
        velnor_actions_orchestrator_core::exclusive_write::create_dir_no_symlink(anchor, parent)?;
    }
    velnor_actions_orchestrator_core::exclusive_write::write_exclusive(&path, &bytes, "request")?;
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

/// Consuming op from one `<op>-request.json` file name.
fn request_op(path: &Path) -> Result<String, OrchestratorError> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let op = name.strip_suffix("-request.json").unwrap_or_default();
    if op == PLAN_OP
        || op == MERGE_OP
        || op == velnor_actions_orchestrator_baseline_publish::baseline_publish::PUBLISH_OP
    {
        Ok(op.to_owned())
    } else {
        Err(internal("unknown_request_op"))
    }
}
