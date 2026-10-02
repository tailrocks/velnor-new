//! Pre-seed closure gate: single plan build plus artifact sharing.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, is_crate_job_id};

use crate::{
    RenderError,
    preseed::{
        PRESEED_BUILD_NAME, PRESEED_DOWNLOAD_NAME, PRESEED_MANIFEST_NAME, PRESEED_STAGE_NAME,
        PRESEED_UPLOAD_NAME, PRESEED_VERIFY_MANIFEST_NAME,
    },
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
    for (name, kind) in [
        (PRESEED_BUILD_NAME, "build"),
        (PRESEED_MANIFEST_NAME, "manifest"),
        (PRESEED_UPLOAD_NAME, "upload"),
        (PRESEED_STAGE_NAME, "stage"),
    ] {
        if !plan.steps.iter().any(|step| step.name == name) {
            return Err(RenderError::InvalidWorkflow(format!(
                "preseed_incomplete:{PLAN_JOB_ID}:{kind}"
            )));
        }
    }
    for (id, job) in jobs {
        if id != FINAL_JOB_ID && id != PUBLISH_JOB_ID && !is_crate_job_id(id) {
            continue;
        }
        let position = |name: &str| job.steps.iter().position(|step| step.name == name);
        let (Some(download_at), Some(verify_at), Some(stage_at)) = (
            position(PRESEED_DOWNLOAD_NAME),
            position(PRESEED_VERIFY_MANIFEST_NAME),
            position(PRESEED_STAGE_NAME),
        ) else {
            let kind = if position(PRESEED_DOWNLOAD_NAME).is_none() {
                "download"
            } else if position(PRESEED_VERIFY_MANIFEST_NAME).is_none() {
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
