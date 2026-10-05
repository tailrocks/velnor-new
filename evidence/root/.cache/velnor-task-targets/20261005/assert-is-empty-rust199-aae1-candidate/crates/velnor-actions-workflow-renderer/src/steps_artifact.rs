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
            (
                "retention-days".to_owned(),
                ARTIFACT_RETENTION_DAYS.to_string(),
            ),
        ]),
    )?;
    step.role = Some(StepRole::MatrixReportUpload);
    Ok(step)
}

/// Baseline-publish upload step display name.
pub const BASELINE_PUBLISH_UPLOAD_NAME: &str = "Upload baseline";
/// Step ID of the baseline-publishing internal step.
pub const PUBLISH_STEP_ID: &str = StepId::PublishBaseline.as_str();
/// Step-output name carrying the derived baseline artifact name.
pub const ARTIFACT_NAME_OUTPUT: &str = "artifact_name";
/// Retention for published baseline artifacts, in days.
///
/// Baselines are cross-run evidence consumed by later runs, so they
/// keep the platform maximum instead of the run-scoped evidence
/// retention; expiry only broadens later lookups to full execution.
pub const BASELINE_RETENTION_DAYS: u32 = 90;

/// Baseline-publish upload step over the derived artifact name.
///
/// Uploads the single `baseline.json` the publish step staged under
/// the exact `velnor-baseline-<commit>-<compat>` name it derived;
/// `if-no-files-found: error` fails closed when the op staged
/// nothing. `if: always()` attaches at render, like every upload.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid action inputs.
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
                format!("${{{{ runner.temp }}}}/velnor/{RUN_KEY_EXPR}/baseline.json"),
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
            (
                "retention-days".to_owned(),
                ARTIFACT_RETENTION_DAYS.to_string(),
            ),
        ]),
    )
}
