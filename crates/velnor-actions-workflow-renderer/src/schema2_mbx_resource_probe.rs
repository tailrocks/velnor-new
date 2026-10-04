//! Per-job MBX resource evidence artifact step.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, StepKind};

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
        bind_export_measurement(job_id, job)?;
        let mut upload = steps::upload_artifact_step(
            &format!("mbx-cache-evidence-{job_id}-{}", steps::RUN_KEY_EXPR),
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

fn bind_export_measurement(job_id: &str, job: &mut Job) -> Result<(), RenderError> {
    let export_indexes: Vec<_> = job
        .steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| {
            (step.name == crate::mbx_bundle::MBX_BUNDLE_EXPORT_NAME).then_some(index)
        })
        .collect();
    if export_indexes.len() > 1 {
        return Err(RenderError::InvalidWorkflow(format!(
            "duplicate_mbx_export_step:{job_id}"
        )));
    }
    let Some(index) = export_indexes.first() else {
        return Ok(());
    };
    let step = &mut job.steps[*index];
    let StepKind::Shell { env, .. } = &mut step.kind else {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_export_step_not_shell:{job_id}"
        )));
    };
    if env
        .get(crate::mbx_bundle::MBX_RESOURCE_EVIDENCE_REQUIRED_ENV)
        .map(String::as_str)
        != Some("false")
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_resource_evidence_binding_missing:{job_id}"
        )));
    }
    env.insert(
        crate::mbx_bundle::MBX_RESOURCE_EVIDENCE_REQUIRED_ENV.to_owned(),
        "true".to_owned(),
    );
    Ok(())
}
