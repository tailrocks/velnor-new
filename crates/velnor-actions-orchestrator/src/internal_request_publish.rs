//! Canonical response paths and artifact publication.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use velnor_actions_contract::{
    FINAL_JSON_FILENAME, FinalStatus, MATRIX_JSON_FILENAME, PLAN_JSON_FILENAME,
    canonical_json_bytes, matrix_json_bytes, plan_json_bytes,
};

use crate::OrchestratorError;
use crate::decisions::plan_artifact_dir;
use crate::internal::{PlanResponse, check_schema, internal, internal_contract};

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
    write_plan_files(&response, velnor_dir, &dir, None)?;
    Ok(dir)
}

/// Write plan artifacts and the cache directives that passed validation.
///
/// The qualification directives are in the same directory as `plan.json`.
/// They can exceed the operating system limit for one environment variable.
/// The `bind-qualification-cache-v1` operation reads the fixed file from the
/// directory named by the plan's run key.
///
/// # Errors
///
/// Gives [`OrchestratorError::Internal`] when the response or directives fail
/// validation, and [`OrchestratorError::Io`] when the destination cannot be
/// written.
pub fn publish_plan_files_with_outputs(
    response_json: &str,
    velnor_dir: &Path,
    outputs: &crate::plan_output_limits::PlanOutputs,
) -> Result<PathBuf, OrchestratorError> {
    let response = PlanResponse::parse(response_json)?;
    if outputs.run_key != response.plan.run_key {
        return Err(internal("plan_output_run_key_mismatch"));
    }
    let dir = plan_artifact_dir(velnor_dir, &response.plan.run_key)?;
    write_plan_files(
        &response,
        velnor_dir,
        &dir,
        Some(&outputs.qualification_cache_directives),
    )?;
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
    qualification_cache_directives: Option<&str>,
) -> Result<(), OrchestratorError> {
    response.validate()?;
    let cache_directive = match (
        response.plan.qualification.is_some(),
        qualification_cache_directives,
    ) {
        (false, None | Some("")) => None,
        (true, Some(value)) if !value.is_empty() => Some(value),
        (false, Some(_)) => return Err(internal("cache_directive_for_non_qualification_plan")),
        (true, _) => return Err(internal("missing_qualification_cache_directive")),
    };
    if cache_directive.is_some_and(|value| {
        value.len() > crate::qualification_cache_runtime::QUALIFICATION_CACHE_DIRECTIVE_MAX_BYTES
            || value.bytes().any(|byte| matches!(byte, b'\n' | b'\r'))
    }) {
        return Err(internal("qualification_cache_directive_bounds"));
    }
    let plan = plan_json_bytes(&response.plan).map_err(internal_contract)?;
    let matrix = matrix_json_bytes(&response.plan.matrix).map_err(internal_contract)?;
    crate::exclusive_write::create_dir_no_symlink(artifact_anchor(velnor_dir)?, dir)?;
    crate::exclusive_write::write_exclusive(&dir.join(PLAN_JSON_FILENAME), &plan, "plan_artifact")?;
    crate::exclusive_write::write_exclusive(
        &dir.join(MATRIX_JSON_FILENAME),
        &matrix,
        "plan_artifact",
    )?;
    if let Some(directive) = cache_directive {
        crate::exclusive_write::write_exclusive(
            &dir.join(crate::qualification_cache_runtime::QUALIFICATION_CACHE_DIRECTIVE_FILENAME),
            directive.as_bytes(),
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
