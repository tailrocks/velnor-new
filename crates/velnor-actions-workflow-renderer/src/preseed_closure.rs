//! Pre-seed closure gate: single plan build plus artifact sharing.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, StepRole, is_crate_job_id};

use crate::{
    RenderError,
    render::{FINAL_JOB_ID, PLAN_JOB_ID, PUBLISH_JOB_ID},
};

/// Pre-seed closure: single plan build plus artifact sharing (Gap A).
///
/// In pre-seed mode the plan job must build, upload, and stage the helper
/// while every present task/final/publish job downloads, digest-verifies,
/// and stages it in that order; anything less would rebuild per job, stage
/// unverified bytes, or invoke an unstaged helper. Outside pre-seed
/// mode there is nothing to close over.
/// # Errors
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
    for (id, job) in jobs {
        if id != FINAL_JOB_ID && id != PUBLISH_JOB_ID && !is_crate_job_id(id) {
            continue;
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
    }
    Ok(())
}
