use std::collections::BTreeMap;

use velnor_actions_contract::{Job, StepRole, is_crate_job_id};

use crate::{
    RenderError,
    render::{FINAL_JOB_ID, PLAN_JOB_ID},
};

pub(crate) fn check_preseed_closure(
    jobs: &BTreeMap<String, Job>,
    preseed: bool,
) -> Result<(), RenderError> {
    if !preseed {
        return Ok(());
    }
    let Some(plan) = jobs.get(PLAN_JOB_ID) else {
        return Ok(());
    };
    check_plan_preseed(plan)?;
    for (id, job) in jobs {
        if id == FINAL_JOB_ID {
            check_final_published_acquire(job)?;
            continue;
        }
        check_consumer_preseed(id, job)?;
    }
    Ok(())
}

fn check_plan_preseed(plan: &Job) -> Result<(), RenderError> {
    for (role, kind) in [
        (StepRole::PreseedBuild, "build"),
        (StepRole::PreseedManifest, "manifest"),
        (StepRole::PreseedUpload, "upload"),
        (StepRole::PreseedStage, "stage"),
    ] {
        if !plan.steps.iter().any(|step| step.role == Some(role)) {
            return Err(RenderError::InvalidWorkflow(format!(
                "preseed_incomplete:{PLAN_JOB_ID}:{kind}"
            )));
        }
    }
    Ok(())
}

fn check_final_published_acquire(job: &Job) -> Result<(), RenderError> {
    let acquisitions = job
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::AcquireVelnor))
        .count();
    if acquisitions != 1
        || job.steps.iter().any(|step| {
            matches!(
                step.role,
                Some(
                    StepRole::PreseedDownload
                        | StepRole::PreseedVerifyManifest
                        | StepRole::PreseedStage
                )
            )
        })
    {
        return Err(RenderError::InvalidWorkflow(
            "preseed_final_requires_published_acquire".to_owned(),
        ));
    }
    Ok(())
}

fn check_consumer_preseed(id: &str, job: &Job) -> Result<(), RenderError> {
    if !is_crate_job_id(id) && job.check_runner.is_none() {
        return Ok(());
    }
    let published_acquisitions = job
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::AcquireVelnor))
        .count();
    if published_acquisitions > 0 {
        if published_acquisitions != 1
            || job.steps.iter().any(|step| {
                matches!(
                    step.role,
                    Some(
                        StepRole::PreseedDownload
                            | StepRole::PreseedVerifyManifest
                            | StepRole::PreseedStage
                    )
                )
            })
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "preseed_published_acquire_conflict:{id}"
            )));
        }
        return Ok(());
    }
    let position = |role| job.steps.iter().position(|step| step.role == Some(role));
    let (Some(download_at), Some(verify_at), Some(stage_at)) = (
        position(StepRole::PreseedDownload),
        position(StepRole::PreseedVerifyManifest),
        position(StepRole::PreseedStage),
    ) else {
        let kind = if position(StepRole::PreseedDownload).is_none() {
            "download"
        } else if position(StepRole::PreseedVerifyManifest).is_none() {
            "verify"
        } else {
            "stage"
        };
        return Err(RenderError::InvalidWorkflow(format!(
            "preseed_incomplete:{id}:{kind}"
        )));
    };
    if !(download_at < verify_at && verify_at < stage_at) {
        return Err(RenderError::InvalidWorkflow(format!(
            "preseed_misordered:{id}"
        )));
    }
    Ok(())
}
