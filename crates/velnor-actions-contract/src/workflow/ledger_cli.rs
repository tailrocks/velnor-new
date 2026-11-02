// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use super::ir::Job;
use super::jobs::{PLAN_JOB_ID, REQUIRED_JOB_ID};
use super::permissions::Permissions;
use super::step::{Step, StepKind};
use super::step_identity::{StepId, StepRole};
use crate::config::WorkflowPolicy;
use crate::errors::ContractError;

pub const LEDGER_CLI_JOB_ID: &str = "ledger-cli";

pub const LEDGER_CLI_DISPLAY_NAME: &str = "Ledger CLI";

pub const LEDGER_CLI_BUILD_STEP: &str = "Build ledger CLI";

pub const LEDGER_CLI_PUBLISH_STEP: &str = "Publish ledger CLI outputs";

pub const LEDGER_CLI_UPLOAD_STEP: &str = "Upload ledger CLI";

pub const LEDGER_CLI_DOWNLOAD_STEP: &str = "Download ledger CLI";

pub const LEDGER_CLI_VERIFY_STEP: &str = "Verify ledger CLI";

pub const LEDGER_CLI_EMIT_STEP: &str = "Emit group ledger";

pub const LEDGER_CLI_ARTIFACT_OUTPUT: &str = "artifact_name";

pub const LEDGER_CLI_SHA256_OUTPUT: &str = "sha256";

pub const LEDGER_CLI_SOURCE_SHA_OUTPUT: &str = "source_sha";

pub const LEDGER_CLI_ARTIFACT_NAME: &str =
    "velnor-ledger-cli-r${{ github.run_id }}-a${{ github.run_attempt }}";

pub const LEDGER_CLI_SHA256_SOURCE: &str = "${{ steps.ledger-cli.outputs.sha256 }}";

pub const LEDGER_CLI_SOURCE_SHA_SOURCE: &str = "${{ steps.ledger-cli.outputs.source_sha }}";

pub const LEDGER_CLI_ARTIFACT_NAME_EXPR: &str = "${{ needs.ledger-cli.outputs.artifact_name }}";

pub const LEDGER_CLI_SHA256_EXPR: &str = "${{ needs.ledger-cli.outputs.sha256 }}";

pub const LEDGER_CLI_SOURCE_SHA_EXPR: &str = "${{ needs.ledger-cli.outputs.source_sha }}";

pub const LEDGER_CLI_SHA256_ENV: &str = "VELNOR_LEDGER_CLI_SHA256";

pub const LEDGER_CLI_SOURCE_SHA_ENV: &str = "VELNOR_LEDGER_CLI_SOURCE_SHA";

pub const LEDGER_CLI_BINARY_PATH: &str = "$RUNNER_TEMP/velnor/ledger-cli/velnor-actions";

pub const LEDGER_CLI_VERIFY_CONDITION: &str = "always()";

pub const LEDGER_CLI_EMIT_CONDITION: &str =
    "always() && steps.verify-ledger-cli.outcome == 'success'";

pub const LEDGER_CLI_RETENTION_DAYS: &str = "1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerCliSource {
    Installed,
    SameRunBuild,
}

impl LedgerCliSource {
    #[must_use]
    pub const fn for_policy(policy: WorkflowPolicy) -> Self {
        match policy {
            WorkflowPolicy::ConsumerV1 => Self::Installed,
            WorkflowPolicy::VelnorRepositoryV1 => Self::SameRunBuild,
        }
    }
}

fn invalid(id: &str, problem: &str) -> ContractError {
    ContractError::identity("job.ledger_cli", format!("{problem}:{id}"))
}

fn positions(job: &Job, keep: impl Fn(&Step) -> bool) -> Vec<usize> {
    job.steps
        .iter()
        .enumerate()
        .filter(|(_, step)| keep(step))
        .map(|(at, _)| at)
        .collect()
}

fn role_positions(job: &Job, role: StepRole) -> Vec<usize> {
    positions(job, |step| step.role == Some(role))
}

fn with_value<'a>(step: &'a Step, key: &str) -> Option<&'a str> {
    match &step.kind {
        StepKind::Action { with, .. } => with.get(key).map(String::as_str),
        _ => None,
    }
}

fn env_value<'a>(step: &'a Step, key: &str) -> Option<&'a str> {
    match &step.kind {
        StepKind::Shell { env, .. } => env.get(key).map(String::as_str),
        _ => None,
    }
}

pub fn validate_ledger_cli_job(job: &Job) -> Result<(), ContractError> {
    let id = LEDGER_CLI_JOB_ID;
    let expected_outputs = BTreeMap::from([
        (
            LEDGER_CLI_ARTIFACT_OUTPUT.to_owned(),
            LEDGER_CLI_ARTIFACT_NAME.to_owned(),
        ),
        (
            LEDGER_CLI_SHA256_OUTPUT.to_owned(),
            LEDGER_CLI_SHA256_SOURCE.to_owned(),
        ),
        (
            LEDGER_CLI_SOURCE_SHA_OUTPUT.to_owned(),
            LEDGER_CLI_SOURCE_SHA_SOURCE.to_owned(),
        ),
    ]);
    let read_only = Permissions::default();
    if !job.needs.is_empty()
        || job.permissions.as_ref().unwrap_or(&read_only) != &read_only
        || job.environment.is_some()
        || !job.services.is_empty()
        || job.check_runner.is_some()
        || job.outputs != expected_outputs
    {
        return Err(invalid(id, "job_shape"));
    }
    validate_ledger_cli_steps(job)
}

fn step_values(step: &Step) -> impl Iterator<Item = &String> {
    let (with, env) = match &step.kind {
        StepKind::Action { with, env, .. } => (Some(with), env),
        StepKind::Shell { env, .. } => (None, env),
        _ => return Vec::new().into_iter(),
    };
    with.into_iter()
        .flat_map(BTreeMap::values)
        .chain(env.values())
        .collect::<Vec<_>>()
        .into_iter()
}

fn single(job: &Job, role: StepRole, id: &str, problem: &str) -> Result<usize, ContractError> {
    match role_positions(job, role)[..] {
        [at] => Ok(at),
        _ => Err(invalid(id, problem)),
    }
}

fn validate_ledger_cli_steps(job: &Job) -> Result<(), ContractError> {
    let id = LEDGER_CLI_JOB_ID;
    let checkout = single(job, StepRole::Checkout, id, "checkout_count")?;
    let build = single(job, StepRole::LedgerCliBuild, id, "build_count")?;
    let publish = single(job, StepRole::LedgerCliPublish, id, "publish_count")?;
    let upload = single(job, StepRole::LedgerCliUpload, id, "upload_count")?;
    let ordered = checkout < build && build < publish && publish < upload;
    let unconditional = [build, publish, upload]
        .into_iter()
        .all(|at| job.steps[at].condition.is_none());
    let step = &job.steps[upload];
    let upload_ok = with_value(step, "name") == Some(LEDGER_CLI_ARTIFACT_NAME)
        && with_value(step, "if-no-files-found") == Some("error")
        && with_value(step, "retention-days") == Some(LEDGER_CLI_RETENTION_DAYS)
        && matches!(with_value(step, "overwrite"), None | Some("false"));
    let secret_free = job.steps.iter().all(|step| {
        step_values(step)
            .all(|value| !value.contains("secrets.") && !value.contains("github.token"))
    });
    if ordered && unconditional && upload_ok && secret_free {
        Ok(())
    } else {
        Err(invalid(id, "step_shape"))
    }
}

pub fn validate_crate_ledger_steps(
    job: &Job,
    id: &str,
    source: LedgerCliSource,
) -> Result<(), ContractError> {
    let [emit] = positions(job, |step| step.name == LEDGER_CLI_EMIT_STEP)[..] else {
        return Err(invalid(id, "emit_count"));
    };
    let download = role_positions(job, StepRole::LedgerCliDownload);
    let verify = role_positions(job, StepRole::LedgerCliVerify);
    let consumes = job.needs.iter().any(|need| need == LEDGER_CLI_JOB_ID);
    let step = &job.steps[emit];
    let ok = match source {
        LedgerCliSource::Installed => {
            download.is_empty()
                && verify.is_empty()
                && !consumes
                && matches!(step.kind, StepKind::Internal { .. })
        }
        LedgerCliSource::SameRunBuild => {
            matches!((download[..].first(), verify[..].first()), (Some(&d), Some(&v))
                if download.len() == 1 && verify.len() == 1 && d < v && v < emit
                    && consumes
                    && job.needs.iter().any(|need| need == PLAN_JOB_ID)
                    && download_ok(&job.steps[d])
                    && verify_ok(&job.steps[v])
                    && emit_ok(step))
        }
    };
    if ok && step.condition.as_deref() == Some(expected_emit_condition(source)) {
        Ok(())
    } else {
        Err(invalid(id, "crate_steps"))
    }
}

fn expected_emit_condition(source: LedgerCliSource) -> &'static str {
    match source {
        LedgerCliSource::Installed => "always()",
        LedgerCliSource::SameRunBuild => LEDGER_CLI_EMIT_CONDITION,
    }
}

fn download_ok(step: &Step) -> bool {
    matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with("actions/download-artifact@"))
        && step.name == LEDGER_CLI_DOWNLOAD_STEP
        && step.condition.as_deref() == Some(LEDGER_CLI_VERIFY_CONDITION)
        && with_value(step, "name") == Some(LEDGER_CLI_ARTIFACT_NAME_EXPR)
}

fn verify_ok(step: &Step) -> bool {
    step.name == LEDGER_CLI_VERIFY_STEP
        && step.id == Some(StepId::LedgerCliVerify)
        && step.condition.as_deref() == Some(LEDGER_CLI_VERIFY_CONDITION)
        && env_value(step, LEDGER_CLI_SHA256_ENV) == Some(LEDGER_CLI_SHA256_EXPR)
        && env_value(step, LEDGER_CLI_SOURCE_SHA_ENV) == Some(LEDGER_CLI_SOURCE_SHA_EXPR)
}

fn emit_ok(step: &Step) -> bool {
    matches!(&step.kind, StepKind::Shell { run, .. } if run[..] == [LEDGER_CLI_BINARY_PATH])
}

pub fn validate_ledger_cli_jobs(
    jobs: &BTreeMap<String, Job>,
    source: LedgerCliSource,
) -> Result<(), ContractError> {
    let emitters: BTreeSet<&str> = jobs
        .iter()
        .filter(|(_, job)| {
            job.steps
                .iter()
                .any(|step| step.name == LEDGER_CLI_EMIT_STEP)
        })
        .map(|(id, _)| id.as_str())
        .collect();
    let builders: Vec<&str> = jobs
        .iter()
        .filter(|(_, job)| !role_positions(job, StepRole::LedgerCliBuild).is_empty())
        .map(|(id, _)| id.as_str())
        .collect();
    let stray = jobs.iter().any(|(id, job)| {
        !emitters.contains(id.as_str()) && job.needs.iter().any(|need| need == LEDGER_CLI_JOB_ID)
    });
    let required_consumes = jobs
        .get(REQUIRED_JOB_ID)
        .is_some_and(|job| job.needs.iter().any(|need| need == LEDGER_CLI_JOB_ID));
    let expected_builders: &[&str] = match source {
        LedgerCliSource::SameRunBuild if !emitters.is_empty() => &[LEDGER_CLI_JOB_ID],
        _ => &[],
    };
    if builders != expected_builders
        || jobs.contains_key(LEDGER_CLI_JOB_ID) == expected_builders.is_empty()
        || stray
        || required_consumes
    {
        return Err(invalid(LEDGER_CLI_JOB_ID, "job_set"));
    }
    if let Some(job) = jobs.get(LEDGER_CLI_JOB_ID) {
        validate_ledger_cli_job(job)?;
    }
    for id in emitters {
        validate_crate_ledger_steps(&jobs[id], id, source)?;
    }
    Ok(())
}
