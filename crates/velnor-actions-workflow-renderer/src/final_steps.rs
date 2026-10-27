// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepRole};

use crate::{
    RenderError,
    render::{CANDIDATE_JOB_ID, FINAL_JOB_ID, RenderContext},
    steps,
};

pub(crate) const FETCH_REPORTS_NAME: &str = "Download every expected matrix artifact";
pub(crate) const PUBLISH_FINAL_NAME: &str = "Publish final report";
pub(crate) const FINAL_ARTIFACT_NAME: &str =
    "velnor-final-r${{ github.run_id }}-a${{ github.run_attempt }}";
pub(crate) const FINAL_ARTIFACT_PATH: &str = "${{ runner.temp }}/velnor/r${{ github.run_id }}-a${{ env.VELNOR_RUN_KEY_ATTEMPT }}/final-report.json";

pub(crate) fn fetch_reports_step() -> Result<Step, RenderError> {
    let mut step = steps::internal_step(FETCH_REPORTS_NAME, steps::FETCH_OPERATION)?;
    step.role = Some(StepRole::FetchReports);
    Ok(step)
}

pub(crate) fn publish_final_report_step() -> Result<Step, RenderError> {
    let mut step = steps::action_step(
        PUBLISH_FINAL_NAME,
        steps::UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            ("name".to_owned(), FINAL_ARTIFACT_NAME.to_owned()),
            ("path".to_owned(), FINAL_ARTIFACT_PATH.to_owned()),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            (
                "retention-days".to_owned(),
                steps::ARTIFACT_RETENTION_DAYS.to_string(),
            ),
        ]),
    )?;
    step.role = Some(StepRole::PublishFinal);
    Ok(step)
}

pub(crate) fn insert_final_fanin(
    jobs: &mut BTreeMap<String, Job>,
    ctx: &RenderContext,
) -> Result<(), RenderError> {
    let candidate = jobs.contains_key(CANDIDATE_JOB_ID);
    let Some(final_job) = jobs.get_mut(FINAL_JOB_ID) else {
        return Ok(());
    };
    if candidate
        && !final_job
            .steps
            .iter()
            .any(|step| step.role == Some(StepRole::AttestationDownload))
    {
        let at = fetch_insert_at(final_job);
        final_job.steps.insert(at, download_attestation_step(ctx)?);
    }
    if !final_job
        .steps
        .iter()
        .any(|step| step.role == Some(StepRole::FetchReports))
    {
        let at = fetch_insert_at(final_job);
        final_job.steps.insert(at, fetch_reports_step()?);
    }
    insert_assemble_step(final_job);
    if !final_job
        .steps
        .iter()
        .any(|step| step.role == Some(StepRole::PublishFinal))
    {
        final_job.steps.push(publish_final_report_step()?);
    }
    Ok(())
}

pub(crate) fn insert_assemble_step(final_job: &mut Job) {
    let existing = final_job.steps.iter().position(|step| {
        step.name == crate::steps_internal::group_ledger_steps::ASSEMBLE_GROUP_LEDGERS_STEP_NAME
    });
    let step = existing.map_or_else(
        crate::steps_internal::group_ledger_steps::assemble_group_ledgers_step,
        |index| final_job.steps.remove(index),
    );
    let at = assemble_insert_at(final_job);
    final_job.steps.insert(at, step);
}

fn assemble_insert_at(job: &Job) -> usize {
    if let Some(at) = job
        .steps
        .iter()
        .position(|step| step.role == Some(StepRole::FetchReports))
    {
        return at + 1;
    }
    fetch_insert_at(job)
}

pub(crate) const ATTESTATION_DOWNLOAD_NAME: &str = "Download candidate attestation";

fn download_attestation_step(ctx: &RenderContext) -> Result<Step, RenderError> {
    let target = velnor_actions_contract::ReleaseTarget::for_runner_label(&ctx.runs_on)
        .ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("unsupported_target_for_runner:{}", ctx.runs_on))
        })?;
    let artifact = crate::artifact_paths::candidate_artifact_name(target.triple())?;
    let path = format!(
        "{}/{}",
        crate::closure::PLAN_ARTIFACT_PATH,
        velnor_actions_contract::CANDIDATE_EVIDENCE_SUBDIR
    );
    let mut step = steps::download_artifact_step(&artifact, &path)?;
    ATTESTATION_DOWNLOAD_NAME.clone_into(&mut step.name);
    step.role = Some(StepRole::AttestationDownload);
    Ok(step)
}

fn fetch_insert_at(job: &Job) -> usize {
    use velnor_actions_contract::StepKind;
    if let Some(at) = job
        .steps
        .iter()
        .position(|step| step.role == Some(StepRole::DownloadPlan))
    {
        return at + 1;
    }
    let want = [steps::WRITE_REQUEST_OPERATION, steps::MERGE_OPERATION].join(":");
    let at = |op: &str| {
        job.steps.iter().position(
            |step| matches!(&step.kind, StepKind::Internal { operation, .. } if operation == op),
        )
    };
    at(&want)
        .or_else(|| at(steps::MERGE_OPERATION))
        .unwrap_or(job.steps.len())
}

// gc1-t56 covers the Required-job fan-in: the assemble step lands after the
// fetch-reports step and before write-request, exactly once.
#[cfg(test)]
mod gc1_t56 {
    use velnor_actions_contract::{Job, JobTimeout, Step};

    use super::{assemble_insert_at, fetch_insert_at, fetch_reports_step, insert_assemble_step};
    use crate::RenderError;
    use crate::steps_internal::group_ledger_steps::ASSEMBLE_GROUP_LEDGERS_STEP_NAME;

    fn required_job(steps: Vec<Step>) -> Job {
        Job {
            display_name: "Required".to_owned(),
            runs_on: "ubuntu-24.04".to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::REQUIRED,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            cache_mode: None,
            environment: None,
            services: Vec::new(),
            outputs: std::collections::BTreeMap::new(),
            steps,
        }
    }

    fn step_names(job: &Job) -> Vec<&str> {
        job.steps.iter().map(|step| step.name.as_str()).collect()
    }

    #[test]
    fn assemble_lands_before_write_request() -> Result<(), RenderError> {
        let mut job = required_job(vec![
            crate::closure::download_plan_step()?,
            fetch_reports_step()?,
            crate::steps_internal::write_request_step(crate::steps::MERGE_OPERATION)?,
            crate::steps::merge_step(),
        ]);
        insert_assemble_step(&mut job);
        assert_eq!(
            step_names(&job),
            vec![
                crate::closure::DOWNLOAD_PLAN_NAME,
                super::FETCH_REPORTS_NAME,
                ASSEMBLE_GROUP_LEDGERS_STEP_NAME,
                "Write request",
                "Merge reports",
            ]
        );
        assert!(assemble_insert_at(&job) > fetch_insert_at(&job));
        insert_assemble_step(&mut job);
        assert_eq!(
            job.steps
                .iter()
                .filter(|step| step.name == ASSEMBLE_GROUP_LEDGERS_STEP_NAME)
                .count(),
            1,
            "assemble inserts exactly once"
        );
        Ok(())
    }
}
