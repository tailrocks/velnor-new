use super::{
    ArtifactReadApi, ReleaseArtifactApi, ReleaseConsumer, ZipProfile, archive_file::SealedArchive,
    checkpoint, inventory, require_consumer_environment, validate_metadata,
};
use crate::{OrchestratorError, internal::internal};
use velnor_actions_contract::PackageReleasePlan;
use velnor_actions_contract::preview_production::{
    ReleaseProductionContext, release_aggregate_artifact_name, validate_release_production_context,
};
use velnor_actions_mise::CheckDeadline;

pub(crate) struct AdmittedReleaseAggregate {
    pub archive: SealedArchive,
}

pub(crate) fn acquire_release_aggregate(
    plan: &PackageReleasePlan,
    context: &ReleaseProductionContext,
    artifact_id: u64,
    sha256: &str,
    directory: &std::path::Path,
    deadline: CheckDeadline,
) -> Result<AdmittedReleaseAggregate, OrchestratorError> {
    require_consumer_environment(ReleaseConsumer::Signer, plan.channel)?;
    let api = ReleaseArtifactApi::new(&plan.repository);
    deadline
        .scope(|| {
            acquire(
                &api,
                plan,
                context,
                artifact_id,
                sha256,
                directory,
                deadline,
            )
        })
        .map_err(|_| internal("preview_artifact_deadline"))?
}

fn acquire(
    api: &impl ArtifactReadApi,
    plan: &PackageReleasePlan,
    context: &ReleaseProductionContext,
    artifact_id: u64,
    digest: &str,
    directory: &std::path::Path,
    deadline: CheckDeadline,
) -> Result<AdmittedReleaseAggregate, OrchestratorError> {
    validate_release_production_context(plan, context)
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
    api.authenticate(plan, context, ReleaseConsumer::Signer)?;
    let records = inventory(api, context.run_id, deadline)?;
    let artifact = records
        .get(&artifact_id)
        .ok_or_else(|| internal("preview_artifact_id_missing"))?;
    let name = release_aggregate_artifact_name(plan, context)
        .map_err(|_| internal("preview_aggregate_context"))?;
    validate_metadata(artifact, artifact_id, digest, &name, plan, context)?;
    if artifact.size_in_bytes > ZipProfile::PreviewAggregate.compressed() {
        return Err(internal("preview_artifact_metadata_identity"));
    }
    let mut archive =
        SealedArchive::create(directory, artifact_id, artifact.size_in_bytes, digest)?;
    api.download(
        artifact_id,
        artifact.size_in_bytes,
        archive.destination_file(),
        deadline,
    )?;
    archive.seal(deadline)?;
    api.authenticate(plan, context, ReleaseConsumer::Signer)?;
    checkpoint(deadline)?;
    Ok(AdmittedReleaseAggregate { archive })
}
