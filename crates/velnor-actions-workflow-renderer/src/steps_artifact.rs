// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepRole};

use super::steps::{
    ARTIFACT_RETENTION_DAYS, CRATE_REPORT_UPLOAD_NAME, DOWNLOAD_ARTIFACT_USES,
    MATRIX_REPORT_UPLOAD_NAME, RUN_KEY_EXPR, UPLOAD_ARTIFACT_USES, action_step,
};
use crate::{RenderError, artifact_paths};

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

pub fn matrix_report_upload_step() -> Result<Step, RenderError> {
    let mut step = matrix_report_upload_raw("${{ matrix.matrix_key }}", MATRIX_REPORT_UPLOAD_NAME)?;
    step.role = Some(StepRole::MatrixReportUpload);
    Ok(step)
}

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
