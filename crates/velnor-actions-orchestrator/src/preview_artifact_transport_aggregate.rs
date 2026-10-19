//! Source-free signer acquisition of the exact current-attempt aggregate ZIP.
use super::{
    ArtifactReadApi, PreviewArtifactApi, PreviewConsumer, checkpoint, inventory,
    require_consumer_environment, sha256, validate_metadata,
};
use crate::{OrchestratorError, internal::internal};
use velnor_actions_contract::PackageReleasePlan;
use velnor_actions_contract::preview_production::{
    PreviewProductionContext, preview_aggregate_artifact_name, validate_preview_production_context,
};
use velnor_actions_mise::CheckDeadline;

/// Service-authenticated aggregate bytes; native manifest/full-package admission follows.
pub(crate) struct AdmittedPreviewAggregate {
    pub bytes: Vec<u8>,
}

/// Acquire a numeric aggregate upload output for the fixed source-free signer.
/// No reduced plan or fabricated assembly request enters this path.
pub(crate) fn acquire_preview_aggregate(
    plan: &PackageReleasePlan,
    context: &PreviewProductionContext,
    artifact_id: u64,
    sha256: &str,
    deadline: CheckDeadline,
) -> Result<AdmittedPreviewAggregate, OrchestratorError> {
    require_consumer_environment(PreviewConsumer::Signer)?;
    let api = PreviewArtifactApi::new(&plan.repository);
    deadline
        .scope(|| acquire(&api, plan, context, artifact_id, sha256, deadline))
        .map_err(|_| internal("preview_artifact_deadline"))?
}

fn acquire(
    api: &impl ArtifactReadApi,
    plan: &PackageReleasePlan,
    context: &PreviewProductionContext,
    artifact_id: u64,
    digest: &str,
    deadline: CheckDeadline,
) -> Result<AdmittedPreviewAggregate, OrchestratorError> {
    validate_preview_production_context(plan, context)
        .map_err(|_| internal("preview_aggregate_context"))?;
    if artifact_id == 0
        || digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(internal("preview_aggregate_reference"));
    }
    checkpoint(deadline)?;
    api.authenticate(plan, context, PreviewConsumer::Signer)?;
    let records = inventory(api, context.run_id, deadline)?;
    let artifact = records
        .get(&artifact_id)
        .ok_or_else(|| internal("preview_artifact_id_missing"))?;
    let name = preview_aggregate_artifact_name(plan, context)
        .map_err(|_| internal("preview_aggregate_context"))?;
    validate_metadata(artifact, artifact_id, digest, &name, plan, context)?;
    let bytes = api.download(artifact_id, artifact.size_in_bytes)?;
    checkpoint(deadline)?;
    if u64::try_from(bytes.len()).ok() != Some(artifact.size_in_bytes) || sha256(&bytes) != digest {
        return Err(internal("preview_artifact_download_identity"));
    }
    api.authenticate(plan, context, PreviewConsumer::Signer)?;
    checkpoint(deadline)?;
    Ok(AdmittedPreviewAggregate { bytes })
}

#[cfg(test)]
#[path = "preview_artifact_transport_aggregate_tests.rs"]
mod tests;
