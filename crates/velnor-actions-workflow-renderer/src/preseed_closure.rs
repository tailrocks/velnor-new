//! Pre-seed closure gate: single plan build plus artifact sharing.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepRole, is_crate_job_id};

use crate::{
    RenderError,
    render::{FINAL_JOB_ID, PLAN_JOB_ID, PUBLISH_JOB_ID},
};

const PLAN_FAILURE_GUARD_NAME: &str = "Require successful pre-seed plan";
const PLAN_FAILURE_GUARD_CONDITION: &str = "always() && needs.plan.result != 'success'";
const DOWNLOAD_PLAN_CONDITION: &str = "success() && needs.plan.result == 'success'";

/// Pre-seed closure: single plan build plus artifact sharing (Gap A).
///
/// When `preseed` is true, the Plan job builds the helper.
/// It uploads the helper and prepares it for use.
/// Each consumer waits for the Plan job.
/// It downloads the helper.
/// It compares the helper digest with the expected digest.
/// It prepares the helper for use.
/// A consumer with `always()` fails before the download if the Plan job fails.
/// This function does not change jobs when `preseed` is false.
/// # Errors
pub(crate) fn check_preseed_closure(
    jobs: &mut BTreeMap<String, Job>,
    preseed: bool,
) -> Result<(), RenderError> {
    if !preseed {
        return Ok(());
    }
    let has_expected_consumers = jobs
        .iter()
        .any(|(id, job)| is_required_consumer_id(id) || has_preseed_consumer_step(job));
    let Some(plan) = jobs.get(PLAN_JOB_ID) else {
        if has_expected_consumers {
            return Err(RenderError::InvalidWorkflow(
                "preseed_plan_missing".to_owned(),
            ));
        }
        return Ok(());
    };
    check_plan_producer(plan)?;
    let consumer_ids = jobs
        .iter()
        .filter(|(id, job)| {
            id.as_str() != PLAN_JOB_ID
                && (is_required_consumer_id(id) || has_preseed_consumer_step(job))
        })
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    for id in consumer_ids {
        let job = jobs
            .get_mut(&id)
            .ok_or_else(|| RenderError::InvalidWorkflow(format!("preseed_job_missing:{id}")))?;
        close_preseed_consumer(job, &id)?;
    }
    Ok(())
}

fn check_plan_producer(plan: &Job) -> Result<(), RenderError> {
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
    if plan.steps.iter().any(|step| {
        matches!(
            step.role,
            Some(StepRole::PreseedDownload | StepRole::PreseedVerifyManifest)
        )
    }) {
        return Err(RenderError::InvalidWorkflow(
            "preseed_plan_consumes_own_artifact".to_owned(),
        ));
    }
    Ok(())
}

fn close_preseed_consumer(job: &mut Job, id: &str) -> Result<(), RenderError> {
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
    if !job.needs.iter().any(|need| need == PLAN_JOB_ID) {
        job.needs.push(PLAN_JOB_ID.to_owned());
    }
    require_successful_plan_for_download(&mut job.steps[download_at]);
    if job
        .condition
        .as_deref()
        .is_some_and(|condition| condition.contains("always()"))
    {
        let guard = plan_failure_guard_step()?;
        if !job.steps.iter().any(|step| step == &guard) {
            job.steps.insert(download_at, guard);
        }
    }
    Ok(())
}

fn has_preseed_consumer_step(job: &Job) -> bool {
    job.steps.iter().any(|step| {
        matches!(
            step.role,
            Some(
                StepRole::PreseedDownload
                    | StepRole::PreseedVerifyManifest
                    | StepRole::PreseedStage
            )
        )
    })
}

fn is_required_consumer_id(id: &str) -> bool {
    id == FINAL_JOB_ID || id == PUBLISH_JOB_ID || is_crate_job_id(id)
}

fn plan_failure_guard_step() -> Result<Step, RenderError> {
    let mut step = crate::steps::shell_step(
        PLAN_FAILURE_GUARD_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), "exit 1".to_owned()],
        BTreeMap::new(),
    )?;
    step.condition = Some(PLAN_FAILURE_GUARD_CONDITION.to_owned());
    Ok(step)
}

fn require_successful_plan_for_download(step: &mut Step) {
    if step.condition.as_deref() == Some(DOWNLOAD_PLAN_CONDITION) {
        return;
    }
    step.condition = Some(match step.condition.take() {
        Some(condition) => format!("({condition}) && {DOWNLOAD_PLAN_CONDITION}"),
        None => DOWNLOAD_PLAN_CONDITION.to_owned(),
    });
}

#[cfg(test)]
#[path = "preseed_closure_tests.rs"]
mod tests;
