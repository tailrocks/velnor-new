//! Artifact upload/download step templates over pinned actions.
//!
//! Split from `steps.rs`: candidate and matrix-report artifact IO
//! share pins, key expressions, and payload shapes.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepId, StepRole};

use super::steps::{
    ARTIFACT_RETENTION_DAYS, CRATE_REPORT_UPLOAD_NAME, DOWNLOAD_ARTIFACT_USES,
    MATRIX_REPORT_UPLOAD_NAME, RUN_KEY_EXPR, UPLOAD_ARTIFACT_USES, action_step,
};
use crate::{RenderError, artifact_paths};

/// Candidate-artifact upload step over the pinned upload action.
/// # Errors
pub fn upload_artifact_step(name: &str, path: &str) -> Result<Step, RenderError> {
    if name.trim().is_empty() || path.trim().is_empty() {
        return Err(RenderError::BadActionRef("empty_artifact_io".to_owned()));
    }
    artifact_paths::check_artifact_path(path)?;
    action_step(
        "Upload candidate",
        UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            ("name".to_owned(), name.to_owned()),
            ("path".to_owned(), path.to_owned()),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            (
                "retention-days".to_owned(),
                ARTIFACT_RETENTION_DAYS.to_string(),
            ),
        ]),
    )
}

/// Candidate-artifact download step over the pinned download action.
/// # Errors
pub fn download_artifact_step(name: &str, path: &str) -> Result<Step, RenderError> {
    if name.trim().is_empty() || path.trim().is_empty() {
        return Err(RenderError::BadActionRef("empty_artifact_io".to_owned()));
    }
    artifact_paths::check_artifact_path(path)?;
    action_step(
        "Download candidate",
        DOWNLOAD_ARTIFACT_USES,
        BTreeMap::from([
            ("name".to_owned(), name.to_owned()),
            ("path".to_owned(), path.to_owned()),
        ]),
    )
}

/// Matrix-report upload step (`velnor-matrix-<run-key>-<matrix-key>`).
///
/// Carries the leg's `matrix-report.json` plus `tasks/` files; the
/// matrix key resolves from the leg's matrix context at runtime, so
/// this step belongs in the matrix task template only. `if: always()`
/// is attached at render.
/// # Errors
pub fn matrix_report_upload_step() -> Result<Step, RenderError> {
    let mut step = matrix_report_upload_raw("${{ matrix.matrix_key }}", MATRIX_REPORT_UPLOAD_NAME)?;
    step.role = Some(StepRole::MatrixReportUpload);
    Ok(step)
}

/// Crate-report upload for one job (`velnor-crate-<run-key>-<job-id>`).
///
/// Carries the job's whole run directory: every entry's
/// `matrix-report.json` plus `tasks/` files. One such step per matrix
/// job (crate jobs and the plan job alike); `if: always()` attaches
/// at render.
/// # Errors
pub fn crate_job_report_upload_step(job_id: &str) -> Result<Step, RenderError> {
    velnor_actions_contract::validate_job_id(job_id).map_err(RenderError::Contract)?;
    let mut step = action_step(
        CRATE_REPORT_UPLOAD_NAME,
        UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            (
                "name".to_owned(),
                format!("velnor-crate-{RUN_KEY_EXPR}-{job_id}"),
            ),
            (
                "path".to_owned(),
                format!("${{{{ runner.temp }}}}/velnor/{RUN_KEY_EXPR}"),
            ),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            // The name binds the plan's attempt: a re-run of this job in a
            // later attempt replaces the earlier report instead of failing
            // on the existing name (hard decision 4, re-run case).
            ("overwrite".to_owned(), "true".to_owned()),
            (
                "retention-days".to_owned(),
                ARTIFACT_RETENTION_DAYS.to_string(),
            ),
        ]),
    )?;
    step.role = Some(StepRole::MatrixReportUpload);
    Ok(step)
}

/// Display name of the baseline upload step.
pub const BASELINE_PUBLISH_UPLOAD_NAME: &str = "Upload baseline";
/// Step ID that provides the published artifact name.
pub const PUBLISH_STEP_ID: &str = StepId::PublishBaseline.as_str();
/// Output name that carries the published artifact name.
pub const ARTIFACT_NAME_OUTPUT: &str = "artifact_name";
/// Retention period for published baseline artifacts.
pub const BASELINE_RETENTION_DAYS: u32 = 90;

/// Upload the baseline manifest that the publisher created.
///
/// # Errors
///
/// Return an error when the action inputs are invalid.
pub fn baseline_publish_upload_step() -> Result<Step, RenderError> {
    action_step(
        BASELINE_PUBLISH_UPLOAD_NAME,
        UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            (
                "name".to_owned(),
                format!("${{{{ steps.{PUBLISH_STEP_ID}.outputs.{ARTIFACT_NAME_OUTPUT} }}}}"),
            ),
            (
                "path".to_owned(),
                format!(
                    "${{{{ runner.temp }}}}/velnor/{RUN_KEY_EXPR}/published-baseline/baseline.json"
                ),
            ),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            (
                "retention-days".to_owned(),
                BASELINE_RETENTION_DAYS.to_string(),
            ),
        ]),
    )
}

/// Matrix-report upload over one key expression plus step name.
fn matrix_report_upload_raw(key: &str, name: &str) -> Result<Step, RenderError> {
    action_step(
        name,
        UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            (
                "name".to_owned(),
                format!("velnor-matrix-{RUN_KEY_EXPR}-{key}"),
            ),
            (
                "path".to_owned(),
                format!("${{{{ runner.temp }}}}/velnor/{RUN_KEY_EXPR}/{key}"),
            ),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            ("overwrite".to_owned(), "true".to_owned()),
            (
                "retention-days".to_owned(),
                ARTIFACT_RETENTION_DAYS.to_string(),
            ),
        ]),
    )
}

/// Suite artifact upload step over the pinned upload action.
///
/// # Errors
///
/// Returns [`RenderError`] when the artifact name, path, or condition is invalid.
pub fn upload_suite_artifact_step(
    step_name: &str,
    artifact_name: &str,
    path: &str,
    retention_days: u32,
    when: velnor_actions_contract::RustSuiteArtifactWhen,
) -> Result<Step, RenderError> {
    if artifact_name.trim().is_empty()
        || path.trim().is_empty()
        || !(1..=90).contains(&retention_days)
    {
        return Err(RenderError::BadActionRef("empty_artifact_io".to_owned()));
    }
    for entry in path.lines() {
        artifact_paths::check_artifact_path(entry)?;
    }
    let mut step = action_step(
        step_name,
        UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            ("name".to_owned(), artifact_name.to_owned()),
            ("path".to_owned(), path.to_owned()),
            ("if-no-files-found".to_owned(), "warn".to_owned()),
            ("retention-days".to_owned(), retention_days.to_string()),
        ]),
    )?;
    step.condition = Some(when.github_condition().to_owned());
    Ok(step)
}

/// Upload a prebuilt nextest archive (`retention-days: 1`, `if-no-files-found: error`).
///
/// # Errors
///
/// Returns [`RenderError`] when the artifact name or path is invalid.
pub fn upload_suite_archive_step(artifact_name: &str, path: &str) -> Result<Step, RenderError> {
    if artifact_name.trim().is_empty() || path.trim().is_empty() {
        return Err(RenderError::BadActionRef("empty_artifact_io".to_owned()));
    }
    artifact_paths::check_artifact_path(path)?;
    let mut step = action_step(
        "Upload suite archive",
        UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            ("name".to_owned(), artifact_name.to_owned()),
            ("path".to_owned(), path.to_owned()),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            ("retention-days".to_owned(), "1".to_owned()),
        ]),
    )?;
    step.condition = Some("success()".to_owned());
    Ok(step)
}

/// Download a prebuilt nextest archive in a sharded suite runner job.
///
/// # Errors
///
/// Returns [`RenderError`] when the artifact name or path is invalid.
pub fn download_suite_archive_step(artifact_name: &str, path: &str) -> Result<Step, RenderError> {
    if artifact_name.trim().is_empty() || path.trim().is_empty() {
        return Err(RenderError::BadActionRef("empty_artifact_io".to_owned()));
    }
    artifact_paths::check_artifact_path(path)?;
    action_step(
        "Download suite archive",
        DOWNLOAD_ARTIFACT_USES,
        BTreeMap::from([
            ("name".to_owned(), artifact_name.to_owned()),
            ("path".to_owned(), path.to_owned()),
        ]),
    )
}
