//! Per-job MBX resource evidence artifact step.

use std::collections::BTreeMap;

use velnor_actions_contract::Job;

use crate::steps;
use crate::{RenderError, schema2::MbxQualificationPins};

const ARTIFACT_STEP: &str = "Upload MBX cache evidence";

/// Add a unique, always-uploaded evidence artifact to each MBX job.
/// # Errors
pub(super) fn attach(
    jobs: &mut BTreeMap<String, Job>,
    _request: &MbxQualificationPins,
) -> Result<(), RenderError> {
    for (job_id, job) in jobs.iter_mut() {
        if !job
            .steps
            .iter()
            .any(|step| step.name == super::mbx_qualification::MBX_CACHE_ACTION_STEP)
        {
            continue;
        }
        let mut upload = steps::upload_artifact_step(
            &format!("mbx-cache-evidence-{job_id}"),
            "${{ runner.temp }}/mbx-cache-evidence",
        )?;
        upload.name.clear();
        upload.name.push_str(ARTIFACT_STEP);
        upload.condition = Some("always()".to_owned());
        if let velnor_actions_contract::StepKind::Action { with, .. } = &mut upload.kind {
            with.insert("if-no-files-found".to_owned(), "error".to_owned());
        }
        job.steps.push(upload);
    }
    Ok(())
}
