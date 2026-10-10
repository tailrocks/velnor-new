//! Data-only resolution of one generated task execution record.
//!
//! The generated composite wrapper owns execution. This boundary validates
//! the checked-in manifest against the renderer marker and the runner-staged
//! plan, then emits one bounded NUL frame without constructing a process.

use std::env;
use std::path::Path;

use crate::safe_read::RepoRead;
use velnor_actions_contract::strict_json::MAX_UNTRUSTED_DOCUMENT_BYTES;
use velnor_actions_contract::workflow::{
    ObligationDecision, TASK_EXECUTION_MANIFEST_PATH, TaskExecutionManifestV1,
    task_execution_manifest_marker_line,
};
use velnor_actions_contract::{
    MAX_TASK_EXECUTION_FRAME_BYTES, canonical_json_bytes, parse_strict_json, validate_digest,
    validate_run_key, validate_task_id,
};

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};

/// Private operation tag for resolving one generated task-execution record.
pub const TASK_EXECUTION_RESOLVER_OP: &str = "resolve-task-execution-v1";
/// Environment key selecting the requested record's execution digest.
pub const TASK_EXECUTION_DIGEST_ENV: &str = "VELNOR_TASK_EXECUTION_DIGEST";
/// Static renderer version embedded in the generated composite action.
pub const GENERATOR_VERSION_ENV: &str = "VELNOR_GENERATOR_VERSION";

/// Resolve the selected task from GitHub's runner environment and emit its
/// NUL-framed data record. This function only reads and validates data.
///
/// # Errors
///
/// Returns an internal or IO error for missing runner context, invalid marker
/// or manifest data, or a mismatch with the staged plan.
pub fn resolve_task_execution() -> Result<Vec<u8>, OrchestratorError> {
    let root = env::var_os("GITHUB_WORKSPACE")
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
        .ok_or_else(|| internal("missing_workspace"))?;
    let runner_temp = env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
        .ok_or_else(|| internal("missing_runner_temp"))?;
    let run_key = crate::internal_request::resolve_run_key(None)?;
    let task_id = required_env(crate::task_report::TASK_ID_ENV, "missing_task_id")?;
    let execution_digest = required_env(TASK_EXECUTION_DIGEST_ENV, "missing_execution_digest")?;
    let generator_version = required_env(GENERATOR_VERSION_ENV, "missing_generator_version")?;
    resolve_task_execution_to(
        &root,
        &runner_temp,
        &run_key,
        &task_id,
        &execution_digest,
        &generator_version,
    )
}

/// Resolve one record using explicit runner inputs (testable core).
///
/// The renderer version is an independent input supplied by the generated
/// composite. The manifest's own version is checked only after its marker has
/// matched that input exactly.
///
/// # Errors
///
/// Returns an internal or IO error for invalid inputs, manifest data, or plan
/// bindings.
pub(crate) fn resolve_task_execution_to(
    root: &Path,
    runner_temp: &Path,
    run_key: &str,
    task_id: &str,
    expected_execution_digest: &str,
    expected_generator_version: &str,
) -> Result<Vec<u8>, OrchestratorError> {
    if !root.is_absolute() || !runner_temp.is_absolute() {
        return Err(internal("runner_paths_must_be_absolute"));
    }
    validate_run_key(run_key).map_err(internal_contract)?;
    validate_task_id(task_id).map_err(internal_contract)?;
    validate_digest(expected_execution_digest).map_err(internal_contract)?;
    let manifest = read_manifest(root, expected_generator_version)?;
    let record = manifest
        .tasks
        .get(task_id)
        .ok_or_else(|| internal("task_not_in_execution_manifest"))?;
    if record.execution_digest != expected_execution_digest {
        return Err(internal("execution_digest_mismatch"));
    }
    let frame = record.nul_frame().map_err(internal_contract)?;
    if frame.len() > MAX_TASK_EXECUTION_FRAME_BYTES {
        return Err(internal("task_execution_frame_too_large"));
    }

    let plan = crate::task_report::load_plan(run_key, runner_temp)?;
    let obligation = plan
        .obligations
        .iter()
        .find(|obligation| obligation.task_id == task_id)
        .ok_or_else(|| internal("task_without_plan_obligation"))?;
    if obligation.decision != ObligationDecision::Execute {
        return Err(internal("task_not_executable_in_plan"));
    }
    let (entry, plan_task_digest) = crate::task_report::entry_and_digest(&plan, task_id)?;
    if record.task_digest != plan_task_digest
        || entry.task_digest != plan_task_digest
        || record.task_id != task_id
        || record.matrix_id != entry.id
        || record.matrix_key != entry.matrix_key
    {
        return Err(internal("task_execution_plan_binding_mismatch"));
    }
    Ok(frame)
}

fn read_manifest(
    root: &Path,
    expected_generator_version: &str,
) -> Result<TaskExecutionManifestV1, OrchestratorError> {
    let expected_marker = task_execution_manifest_marker_line(expected_generator_version)
        .map_err(internal_contract)?;
    let raw = match crate::safe_read::read_repo_file(
        root,
        TASK_EXECUTION_MANIFEST_PATH,
        MAX_UNTRUSTED_DOCUMENT_BYTES as u64,
    )? {
        RepoRead::Absent => return Err(internal("missing_task_execution_manifest")),
        RepoRead::Text(text) => text,
    };
    let (marker, body_with_newline) = raw
        .split_once('\n')
        .ok_or_else(|| internal("malformed_task_execution_marker"))?;
    if marker != expected_marker {
        return Err(internal("task_execution_marker_mismatch"));
    }
    let body = body_with_newline
        .strip_suffix('\n')
        .ok_or_else(|| internal("malformed_task_execution_document"))?;
    if body.bytes().any(|byte| matches!(byte, b'\n' | b'\r')) {
        return Err(internal("noncanonical_task_execution_document"));
    }
    let value =
        parse_strict_json(body).map_err(|_| internal("unparsable_task_execution_manifest"))?;
    let manifest: TaskExecutionManifestV1 = serde_json::from_value(value)
        .map_err(|_| internal("unparsable_task_execution_manifest"))?;
    manifest.validate().map_err(internal_contract)?;
    if manifest.generator_version != expected_generator_version {
        return Err(internal("task_execution_version_mismatch"));
    }
    let canonical = canonical_json_bytes(&manifest).map_err(internal_contract)?;
    if canonical != body.as_bytes() {
        return Err(internal("noncanonical_task_execution_manifest"));
    }
    Ok(manifest)
}

fn required_env(key: &str, problem: &str) -> Result<String, OrchestratorError> {
    env::var(key)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal(problem))
}

#[cfg(test)]
#[path = "task_execution_tests.rs"]
mod tests;
