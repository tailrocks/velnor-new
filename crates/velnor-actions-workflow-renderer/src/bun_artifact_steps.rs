//! Artifact steps of sharded Bun suites: per-leg blob and failure results,
//! the report job's blob download and its merged-report upload.
//!
//! Names derive from the job id and the leg ordinal, so they are unique in
//! the run. Every upload sets `overwrite: true`: "Re-run failed jobs" replaces
//! the earlier attempt's artifact of the same leg, and the re-run report job
//! merges the latest blob per leg. Artifacts never cross runs.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};

use crate::RenderError;
use crate::steps::{DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES, action_step};

/// Prefix of every artifact name a Bun suite job uploads or downloads.
pub const BUN_ARTIFACT_PREFIX: &str = "velnor-pw-";
/// Runner-temp directory a leg writes its blob into.
pub const BUN_BLOB_DIR: &str = "${{ runner.temp }}/velnor/pw-blob";
/// Runner-temp directory the report job downloads every leg blob into.
pub const BUN_BLOBS_DIR: &str = "${{ runner.temp }}/velnor/pw-blobs";
/// Runner-temp directory the report job writes the merged report into.
pub const BUN_REPORT_DIR: &str = "${{ runner.temp }}/velnor/pw-report";
/// Leg blobs only feed the same run's report job.
pub(crate) const BUN_BLOB_RETENTION_DAYS: u16 = 1;
/// Condition of the blob steps: upload even when tests failed.
pub const NOT_CANCELLED: &str = "!cancelled()";
/// Condition of the failure-results upload.
pub(crate) const ON_FAILURE: &str = "failure()";

fn upload(
    name: &str,
    artifact: String,
    path: String,
    missing: &str,
    retention: u16,
) -> Result<Step, RenderError> {
    action_step(
        name,
        UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            ("name".to_owned(), artifact),
            ("path".to_owned(), path),
            ("if-no-files-found".to_owned(), missing.to_owned()),
            ("overwrite".to_owned(), "true".to_owned()),
            ("retention-days".to_owned(), retention.to_string()),
        ]),
    )
}

/// Upload of one leg's blob (`velnor-pw-blob-<job-id>-leg-<n>`).
///
/// # Errors
///
/// Returns render errors for an invalid step.
pub fn bun_blob_upload_step(job_id: &str) -> Result<Step, RenderError> {
    let mut step = upload(
        "Upload Playwright blob",
        format!("{BUN_ARTIFACT_PREFIX}blob-{job_id}-leg-${{{{ matrix.leg }}}}"),
        format!("{BUN_BLOB_DIR}/leg-${{{{ matrix.leg }}}}.zip"),
        "error",
        BUN_BLOB_RETENTION_DAYS,
    )?;
    step.condition = Some(NOT_CANCELLED.to_owned());
    Ok(step)
}

/// Upload of a failed leg's `test-results` (traces, screenshot diffs).
///
/// # Errors
///
/// Returns render errors for an invalid step.
pub fn bun_results_upload_step(
    job_id: &str,
    member_dir: &str,
    retention: u16,
) -> Result<Step, RenderError> {
    let dir = if member_dir.is_empty() {
        "test-results".to_owned()
    } else {
        format!("{member_dir}/test-results")
    };
    let mut step = upload(
        "Upload Playwright results",
        format!("{BUN_ARTIFACT_PREFIX}results-{job_id}-leg-${{{{ matrix.leg }}}}"),
        dir,
        "warn",
        retention,
    )?;
    step.condition = Some(ON_FAILURE.to_owned());
    Ok(step)
}

/// The report job's download of every leg blob of `suite_job_id`.
///
/// # Errors
///
/// Returns render errors for an invalid step.
pub fn bun_blobs_download_step(suite_job_id: &str) -> Result<Step, RenderError> {
    action_step(
        "Download Playwright blobs",
        DOWNLOAD_ARTIFACT_USES,
        BTreeMap::from([
            (
                "pattern".to_owned(),
                format!("{BUN_ARTIFACT_PREFIX}blob-{suite_job_id}-leg-*"),
            ),
            ("merge-multiple".to_owned(), "true".to_owned()),
            ("path".to_owned(), BUN_BLOBS_DIR.to_owned()),
        ]),
    )
}

/// The report job's upload of the merged HTML and JSON report.
///
/// # Errors
///
/// Returns render errors for an invalid step.
pub fn bun_report_upload_step(suite_job_id: &str, retention: u16) -> Result<Step, RenderError> {
    upload(
        "Upload Playwright report",
        format!("{BUN_ARTIFACT_PREFIX}report-{suite_job_id}"),
        BUN_REPORT_DIR.to_owned(),
        "error",
        retention,
    )
}

/// True for one of the pinned artifact actions above, by its generated shape:
/// no role, the pinned upload or download action, a `velnor-pw-` name or
/// pattern. The job reconstruction then compares every input byte for byte.
pub(crate) fn is_bun_artifact_step(step: &Step) -> bool {
    let StepKind::Action { uses, with, .. } = &step.kind else {
        return false;
    };
    let named = |key: &str| {
        with.get(key)
            .is_some_and(|name| name.starts_with(BUN_ARTIFACT_PREFIX))
    };
    step.role.is_none()
        && ((uses == UPLOAD_ARTIFACT_USES && named("name"))
            || (uses == DOWNLOAD_ARTIFACT_USES && named("pattern") && !with.contains_key("name")))
}
