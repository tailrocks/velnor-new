// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};

use crate::RenderError;
use crate::steps::{DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES, action_step};

pub const BUN_ARTIFACT_PREFIX: &str = "velnor-pw-";
pub const BUN_BLOB_DIR: &str = "${{ runner.temp }}/velnor/pw-blob";
pub const BUN_BLOBS_DIR: &str = "${{ runner.temp }}/velnor/pw-blobs";
pub const BUN_REPORT_DIR: &str = "${{ runner.temp }}/velnor/pw-report";
pub(crate) const BUN_BLOB_RETENTION_DAYS: u16 = 1;
pub const NOT_CANCELLED: &str = "!cancelled()";
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

pub fn bun_report_upload_step(suite_job_id: &str, retention: u16) -> Result<Step, RenderError> {
    upload(
        "Upload Playwright report",
        format!("{BUN_ARTIFACT_PREFIX}report-{suite_job_id}"),
        BUN_REPORT_DIR.to_owned(),
        "error",
        retention,
    )
}

pub const BUN_ASSETS_DIR: &str = ".velnor/deps";
pub const BUN_ASSETS_ARCHIVE_PREFIX: &str = "velnor-pw-assets-";
pub const BUN_ASSETS_STAGE_DIR: &str = "${{ runner.temp }}/velnor/bun-assets";

#[must_use]
pub fn bun_assets_archive_name(member_job_id: &str) -> String {
    format!("{BUN_ASSETS_ARCHIVE_PREFIX}{member_job_id}.tar.gz")
}

pub fn bun_assets_upload_step(member_job_id: &str) -> Result<Step, RenderError> {
    let archive = bun_assets_archive_name(member_job_id);
    action_step(
        "Upload private assets",
        UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            (
                "path".to_owned(),
                format!("{BUN_ASSETS_STAGE_DIR}/{member_job_id}/{archive}"),
            ),
            ("archive".to_owned(), "false".to_owned()),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            ("overwrite".to_owned(), "true".to_owned()),
            (
                "retention-days".to_owned(),
                BUN_BLOB_RETENTION_DAYS.to_string(),
            ),
        ]),
    )
}

pub fn bun_assets_download_step(member_job_id: &str) -> Result<Step, RenderError> {
    let archive = bun_assets_archive_name(member_job_id);
    action_step(
        "Download private assets",
        DOWNLOAD_ARTIFACT_USES,
        BTreeMap::from([
            ("name".to_owned(), archive),
            (
                "path".to_owned(),
                format!("{BUN_ASSETS_STAGE_DIR}/{member_job_id}"),
            ),
            ("digest-mismatch".to_owned(), "error".to_owned()),
        ]),
    )
}

pub(crate) fn is_bun_artifact_step(step: &Step) -> bool {
    let StepKind::Action { uses, with, .. } = &step.kind else {
        return false;
    };
    let named = |key: &str| {
        with.get(key)
            .is_some_and(|name| name.starts_with(BUN_ARTIFACT_PREFIX))
    };
    let assets_archive = with
        .get("path")
        .is_some_and(|path| path.contains("/velnor-pw-assets-") && path.ends_with(".tar.gz"));
    let assets_download = with
        .get("name")
        .is_some_and(|name| name.starts_with(BUN_ASSETS_ARCHIVE_PREFIX));
    step.role.is_none()
        && ((uses == UPLOAD_ARTIFACT_USES && (named("name") || assets_archive))
            || (uses == DOWNLOAD_ARTIFACT_USES
                && ((named("pattern") && !with.contains_key("name"))
                    || (assets_download && !with.contains_key("pattern")))))
}
