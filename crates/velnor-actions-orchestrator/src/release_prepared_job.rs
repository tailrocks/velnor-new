//! Fresh credential-free original archive producer; no whole-workflow compilation.
use super::{JobInputs, ProofKind};
use crate::OrchestratorError;
use crate::release_emit::release_source_artifact_input::CompiledSourceArtifactInput;
use velnor_actions_contract::{JobTimeout, StepId, ToolCacheDomain};
use velnor_actions_workflow_renderer::{
    release_artifact_channels::{job_outputs, upload_step},
    release_jobs::{ReleaseJobSpec, ReleaseRole},
    release_permissions::JobPermissions,
    source_helper::source_helper_step,
};

/// Reconstruct the complete producer from actual source and anonymous SDK owners.
pub(crate) fn prepared_package_job(
    inputs: &JobInputs<'_>,
) -> Result<ReleaseJobSpec, OrchestratorError> {
    let role = ReleaseRole::PackagePreparedAnonymous;
    let records = super::reconcile::source_intent_control_tool_records(inputs)?;
    let source_full: Vec<_> = super::reconcile::source_snapshot_tool_records(inputs)?
        .into_iter()
        .filter(|record| {
            record
                .environment()
                .get("MISE_DATA_DIR")
                .map(String::as_str)
                == Some(ToolCacheDomain::Full.root())
        })
        .collect();
    if records != source_full {
        return Err(OrchestratorError::Contract {
            problem: "release_prepared_source_tool_authority".to_owned(),
        });
    }
    let source = CompiledSourceArtifactInput::compile(inputs)?;
    let mut steps = records
        .iter()
        .map(|record| {
            source_helper_step(
                "Prepare qualified anonymous tools",
                record,
                record.environment().clone(),
            )
            .map_err(Into::into)
        })
        .collect::<Result<Vec<_>, OrchestratorError>>()?;
    steps.push(source.download_step()?);
    let mut prepared = super::proof_step(inputs, ProofKind::PreparedPackage)?;
    prepared.id = Some(StepId::new("release-package")?);
    steps.extend([prepared, upload_step(role)?]);
    Ok(ReleaseJobSpec {
        role,
        display_name: role.job_id().to_owned(),
        runs_on: inputs.label.to_owned(),
        timeout_minutes: JobTimeout::RELEASE,
        needs: vec![ReleaseRole::SourceSnapshotForge.job_id().to_owned()],
        condition: None,
        environment: None,
        permissions: JobPermissions::expected(role),
        steps,
        outputs: job_outputs(role)?,
    })
}
