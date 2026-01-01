//! Data-only resolution of one generated task execution record.
//!
//! The generated composite wrapper owns execution. This boundary validates
//! the checked-in manifest against the renderer marker and the runner-staged
//! plan, then emits one bounded NUL frame without constructing a process.

use std::env;
use std::path::Path;
use std::path::PathBuf;

use crate::safe_read::RepoRead;
use velnor_actions_contract::strict_json::MAX_UNTRUSTED_DOCUMENT_BYTES;
use velnor_actions_contract::workflow::{
    ObligationDecision, TASK_EXECUTION_FRAME_MAGIC, TASK_EXECUTION_MANIFEST_PATH,
    TaskExecutionManifestEntryV1, TaskExecutionManifestV1, task_execution_manifest_marker_line,
};
use velnor_actions_contract::{
    MAX_TASK_EXECUTION_ARGV, MAX_TASK_EXECUTION_ENV, MAX_TASK_EXECUTION_FRAME_BYTES,
    canonical_json_bytes, parse_strict_json, validate_digest, validate_run_key,
};

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};

/// Private operation tag for resolving one generated task-execution record.
pub const TASK_EXECUTION_RESOLVER_OP: &str = "resolve-task-execution-v1";
/// Environment key selecting the requested record's execution digest.
pub const TASK_EXECUTION_DIGEST_ENV: &str = "VELNOR_TASK_EXECUTION_DIGEST";
/// Static renderer version embedded in the generated composite action.
pub const GENERATOR_VERSION_ENV: &str = "VELNOR_GENERATOR_VERSION";
/// Static runner-temp value embedded in the generated composite action.
pub const RUNTIME_RUNNER_TEMP_ENV: &str = "VELNOR_RUNTIME_RUNNER_TEMP";

const RUNNER_TEMP_ENV: &str = "RUNNER_TEMP";
const TASK_EXECUTION_FRAME_END: &str = "END";
const RUNNER_TEMP_EXPRESSION: &str = "${{ runner.temp }}";

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
    let runner_temp = env::var_os(RUNNER_TEMP_ENV)
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
        .ok_or_else(|| internal("missing_runner_temp"))?;
    let action_runner_temp = env::var_os(RUNTIME_RUNNER_TEMP_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| internal("missing_runtime_runner_temp"))?;
    validate_runner_temp_binding(&runner_temp, &action_runner_temp)?;
    let run_key = crate::internal_request::resolve_run_key(None)?;
    let execution_digest = required_env(TASK_EXECUTION_DIGEST_ENV, "missing_execution_digest")?;
    let generator_version = required_env(GENERATOR_VERSION_ENV, "missing_generator_version")?;
    resolve_task_execution_to(
        &root,
        &runner_temp,
        &run_key,
        &execution_digest,
        &generator_version,
    )
}

/// Resolve one record using the full execution digest as the caller selector.
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
    expected_execution_digest: &str,
    expected_generator_version: &str,
) -> Result<Vec<u8>, OrchestratorError> {
    if !root.is_absolute() {
        return Err(internal("runner_paths_must_be_absolute"));
    }
    validate_runner_temp_path(runner_temp)?;
    validate_run_key(run_key).map_err(internal_contract)?;
    validate_digest(expected_execution_digest).map_err(internal_contract)?;
    let manifest = read_manifest(root, expected_generator_version)?;
    let record = unique_record_by_execution_digest(&manifest, expected_execution_digest)?;
    let task_id = record.task_id.as_str();
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
        || record.matrix_id != entry.id
        || record.matrix_key != entry.matrix_key
    {
        return Err(internal("task_execution_plan_binding_mismatch"));
    }

    // The manifest digest and existing plan digest bind the original literal
    // expression. Materialize it only after both bindings have been checked.
    encode_runtime_frame(record, runner_temp)
}

/// Select a single validated manifest record using its full execution digest.
///
/// A collision must fail closed instead of depending on map iteration order.
fn unique_record_by_execution_digest<'a>(
    manifest: &'a TaskExecutionManifestV1,
    expected_execution_digest: &str,
) -> Result<&'a TaskExecutionManifestEntryV1, OrchestratorError> {
    let mut matching = manifest
        .tasks
        .values()
        .filter(|record| record.execution_digest == expected_execution_digest);
    let Some(record) = matching.next() else {
        return Err(internal("execution_digest_mismatch"));
    };
    if matching.next().is_some() {
        return Err(internal("ambiguous_execution_digest"));
    }
    Ok(record)
}

fn validate_runner_temp_binding(
    runner_temp: &Path,
    action_runner_temp: &Path,
) -> Result<(), OrchestratorError> {
    validate_runner_temp_path(runner_temp)?;
    validate_runner_temp_path(action_runner_temp)?;
    if runner_temp != action_runner_temp {
        return Err(internal("runner_temp_binding_mismatch"));
    }
    Ok(())
}

fn validate_runner_temp_path(path: &Path) -> Result<&str, OrchestratorError> {
    let Some(value) = path.to_str() else {
        return Err(internal("invalid_runner_temp"));
    };
    if !path.is_absolute() {
        return Err(internal("runner_paths_must_be_absolute"));
    }
    if value.is_empty()
        || value.contains('\0')
        || value.contains('\n')
        || value.contains('\r')
        || value.contains("${{")
    {
        return Err(internal("invalid_runner_temp"));
    }
    Ok(value)
}

fn encode_runtime_frame(
    record: &TaskExecutionManifestEntryV1,
    runner_temp: &Path,
) -> Result<Vec<u8>, OrchestratorError> {
    record.validate().map_err(internal_contract)?;
    let runner_temp = validate_runner_temp_path(runner_temp)?;
    if record.argv.len() > MAX_TASK_EXECUTION_ARGV || record.env.len() > MAX_TASK_EXECUTION_ENV {
        return Err(internal("task_execution_frame_too_large"));
    }

    let mut fields = Vec::with_capacity(13 + record.argv.len() + record.env.len() * 2);
    fields.extend([
        TASK_EXECUTION_FRAME_MAGIC.to_owned(),
        record.task_id.clone(),
        record.execution_digest.clone(),
        record.task_digest.clone(),
        record.matrix_id.clone(),
        record.matrix_key.clone(),
        record.report_helper_version.clone(),
        u8::from(record.matrix_max_parallel.is_some()).to_string(),
        record
            .matrix_max_parallel
            .map_or_else(String::new, |cap| cap.to_string()),
        record.argv.len().to_string(),
    ]);
    for value in &record.argv {
        fields.push(resolve_runner_temp_expression(value, runner_temp)?);
    }
    fields.push(record.env.len().to_string());
    for (key, value) in &record.env {
        fields.push(key.clone());
        fields.push(resolve_runner_temp_expression(value, runner_temp)?);
    }
    fields.push(TASK_EXECUTION_FRAME_END.to_owned());

    let mut bytes = Vec::new();
    for field in fields {
        if field.contains('\0') {
            return Err(internal("task_execution_frame_nul_field"));
        }
        bytes.extend_from_slice(field.as_bytes());
        bytes.push(0);
    }
    if bytes.len() > MAX_TASK_EXECUTION_FRAME_BYTES {
        return Err(internal("task_execution_frame_too_large"));
    }
    Ok(bytes)
}

fn resolve_runner_temp_expression(
    value: &str,
    runner_temp: &str,
) -> Result<String, OrchestratorError> {
    let mut resolved = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find("${{") {
        resolved.push_str(&rest[..start]);
        let expression = &rest[start..];
        let Some(end) = expression.find("}}") else {
            return Err(internal("malformed_task_execution_expression"));
        };
        if &expression[..end + 2] != RUNNER_TEMP_EXPRESSION {
            return Err(internal("unsupported_task_execution_expression"));
        }
        resolved.push_str(runner_temp);
        rest = &expression[end + 2..];
    }
    resolved.push_str(rest);
    if resolved.contains("${{") {
        return Err(internal("unresolved_task_execution_expression"));
    }
    Ok(resolved)
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
