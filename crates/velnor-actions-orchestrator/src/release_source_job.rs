//! Fresh source-only producer reconstruction; never compiles the surrounding graph.
use super::{JobInputs, ProofKind};
use crate::OrchestratorError;
use velnor_actions_contract::{JobTimeout, StepId};
use velnor_actions_workflow_renderer::{
    release_artifact_channels::{job_outputs, upload_step},
    release_jobs::{ReleaseJobSpec, ReleaseRole},
    release_permissions::JobPermissions,
    source_helper::source_helper_step,
};

/// Reconstruct the complete qualified source producer from its actual owners.
pub(crate) fn source_snapshot_job(
    inputs: &JobInputs<'_>,
) -> Result<ReleaseJobSpec, OrchestratorError> {
    let role = ReleaseRole::SourceSnapshotForge;
    let records = super::reconcile::source_snapshot_tool_records(inputs)?;
    if records != crate::release_emit::release_admission::admission_tool_records(inputs)? {
        return Err(OrchestratorError::Contract {
            problem: "release_source_admission_tool_authority".to_owned(),
        });
    }
    let mut steps = records
        .iter()
        .map(|record| {
            source_helper_step(
                "Prepare qualified source tools",
                record,
                record.environment().clone(),
            )
            .map_err(Into::into)
        })
        .collect::<Result<Vec<_>, OrchestratorError>>()?;
    steps.push(super::admission_vectors::forge_admission(inputs)?);
    let mut snapshot = super::proof_step(inputs, ProofKind::SourceSnapshot)?;
    snapshot.id = Some(StepId::new("release-source-snapshot")?);
    steps.extend([snapshot, upload_step(role)?]);
    Ok(ReleaseJobSpec {
        role,
        display_name: role.job_id().to_owned(),
        runs_on: inputs.label.to_owned(),
        timeout_minutes: JobTimeout::RELEASE,
        needs: Vec::new(),
        condition: None,
        environment: None,
        permissions: JobPermissions::expected(role),
        steps,
        outputs: job_outputs(role)?,
    })
}
