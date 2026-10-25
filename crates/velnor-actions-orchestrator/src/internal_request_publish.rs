//! Canonical plan/final artifact publication for internal operations.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use velnor_actions_contract::{
    FINAL_JSON_FILENAME, FinalStatus, MATRIX_JSON_FILENAME, PLAN_JSON_FILENAME,
    canonical_json_bytes, matrix_json_bytes, plan_json_bytes,
};

use crate::OrchestratorError;
use crate::decisions::plan_artifact_dir;
use crate::internal::{PlanResponse, check_schema, internal, internal_contract};

/// Publish `plan.json` and `matrix.json` for a non-qualification plan.
///
/// Qualification plans require the typed directive outputs passed to
/// [`publish_plan_files_with_outputs`].
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for malformed responses or
/// missing qualification directives, and [`OrchestratorError::Io`] for
/// unwritable directories.
pub fn publish_plan_files(
    response_json: &str,
    velnor_dir: &Path,
) -> Result<PathBuf, OrchestratorError> {
    let response = PlanResponse::parse(response_json)?;
    let dir = plan_artifact_dir(velnor_dir, &response.plan.run_key)?;
    write_plan_files(&response, velnor_dir, &dir, None)?;
    Ok(dir)
}

/// Publish plan artifacts and the validated qualification-cache directives.
///
/// The before-snapshot must already be staged in the run directory. This
/// binds the planner's outputs to that exact snapshot before the plan
/// artifact becomes visible.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for response, run-key, snapshot,
/// or directive validation failures, and [`OrchestratorError::Io`] when the
/// destination cannot be written.
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

/// Write canonical plan artifacts and, for qualification plans, directives.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for encoding or qualification
/// validation failures, and [`OrchestratorError::Io`] for unwritable paths.
fn write_plan_files(
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
    if let Some(context) = response.plan.qualification.as_ref() {
        crate::qualification_cache_backend::verify_staged_qualification_cache_before(
            dir,
            context,
            &response.plan.run_key,
        )?;
    }
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

/// Publish the canonical `final-report.json` for the final artifact.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for malformed responses or
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
fn artifact_anchor(velnor_dir: &Path) -> Result<&Path, OrchestratorError> {
    velnor_dir
        .parent()
        .ok_or_else(|| internal("missing_dir_anchor"))
}
