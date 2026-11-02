// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::artifacts::PLAN_ARTIFACT_NAME;
use super::ir::Job;
use super::ledger_cli::{
    LEDGER_CLI_ARTIFACT_NAME_EXPR, LEDGER_CLI_BINARY_PATH, invalid, with_value,
};
use super::step::{Step, StepKind};
use super::step_identity::StepRole;
use crate::errors::ContractError;

const DOWNLOAD_ACTION_PREFIX: &str = "actions/download-artifact@";

const UPLOAD_ACTION_PREFIX: &str = "actions/upload-artifact@";

const RUNNER_TEMP_EXPR: &str = "${{ runner.temp }}";

const RUNNER_TEMP_ENV: &str = "$RUNNER_TEMP";

fn action_prefix(step: &Step, prefix: &str) -> bool {
    matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with(prefix))
}

fn normalized(path: &str) -> String {
    path.replace(RUNNER_TEMP_EXPR, RUNNER_TEMP_ENV)
}

fn executes_download(step: &Step, download_dirs: &[String]) -> bool {
    let StepKind::Shell { run, .. } = &step.kind else {
        return false;
    };
    if run[..] == [LEDGER_CLI_BINARY_PATH] {
        return false;
    }
    run.first().is_some_and(|program| {
        let program = normalized(program);
        download_dirs
            .iter()
            .any(|dir| program.starts_with(&format!("{dir}/")))
    })
}

fn allowed_download(step: &Step) -> bool {
    let expected = match step.role {
        Some(StepRole::DownloadPlan) => PLAN_ARTIFACT_NAME,
        Some(StepRole::LedgerCliDownload) => LEDGER_CLI_ARTIFACT_NAME_EXPR,
        _ => return false,
    };
    with_value(step, "name") == Some(expected)
}

fn role_count(downloads: &[&Step], role: StepRole) -> usize {
    downloads
        .iter()
        .filter(|step| step.role == Some(role))
        .count()
}

pub(super) fn validate_artifact_boundary(job: &Job, id: &str) -> Result<(), ContractError> {
    let downloads: Vec<&Step> = job
        .steps
        .iter()
        .filter(|step| action_prefix(step, DOWNLOAD_ACTION_PREFIX))
        .collect();
    if !downloads.iter().all(|step| allowed_download(step)) {
        return Err(invalid(id, "extra_artifact_download"));
    }
    let repeated = [StepRole::DownloadPlan, StepRole::LedgerCliDownload]
        .into_iter()
        .any(|role| role_count(&downloads, role) > 1);
    if repeated {
        return Err(invalid(id, "repeated_artifact_download"));
    }
    let uploads_ok = job
        .steps
        .iter()
        .filter(|step| action_prefix(step, UPLOAD_ACTION_PREFIX))
        .all(|step| step.role == Some(StepRole::MatrixReportUpload));
    if !uploads_ok {
        return Err(invalid(id, "extra_artifact_upload"));
    }
    let download_dirs: Vec<String> = downloads
        .iter()
        .filter_map(|step| with_value(step, "path"))
        .map(normalized)
        .collect();
    if job
        .steps
        .iter()
        .any(|step| executes_download(step, &download_dirs))
    {
        return Err(invalid(id, "downloaded_artifact_executed"));
    }
    Ok(())
}
