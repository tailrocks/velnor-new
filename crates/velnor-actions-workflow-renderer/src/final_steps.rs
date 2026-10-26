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
    if !final_job
        .steps
        .iter()
        .any(|step| step.role == Some(StepRole::PublishFinal))
    {
        final_job.steps.push(publish_final_report_step()?);
    }
    Ok(())
}

pub(crate) const ATTESTATION_DOWNLOAD_NAME: &str = "Download candidate attestation";

fn download_attestation_step(ctx: &RenderContext) -> Result<Step, RenderError> {
    let target = velnor_actions_contract::ReleaseTarget::for_runner_label(&ctx.runs_on)
        .ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("unsupported_target_for_runner:{}", ctx.runs_on))
        })?;
    let artifact = crate::candidate_artifact_name(target.triple())?;
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
