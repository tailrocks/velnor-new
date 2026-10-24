use super::{RecoveryContext, RecoveryState, quarantine_context};
use crate::error::HostError;
use crate::launch::resource_probe::{
    projection::DockerRoot, publication::PublicationRecoveryProjection,
};

pub(super) async fn load_recovery_state(
    context: &RecoveryContext<'_>,
    root: &DockerRoot,
) -> Result<RecoveryState, HostError> {
    let instance_id = context
        .deadline
        .within(context.journal.instance_id())
        .await??;
    let stored = load_stored_recovery(context).await?;
    let recovery = decode_recovery(context, &stored).await?;
    if !stored_matches(context, &stored, &recovery, &instance_id) {
        quarantine_context(context).await?;
        return Err(HostError::Ownership);
    }
    let volume_name = recovery.metadata_volume_name().to_owned();
    let volume_mountpoint = recovery.metadata_volume_mountpoint().to_owned();
    let volume_id = recovery.metadata_volume_id().to_owned();
    let metadata_operation_id = recovery.metadata_operation_id();
    let projection = recovery.into_projection(root.path())?;
    verify_metadata_volume(
        context,
        &stored,
        &volume_name,
        &volume_id,
        &volume_mountpoint,
        &metadata_operation_id,
    )
    .await?;
    Ok(RecoveryState {
        projection,
        volume_name,
        volume_mountpoint,
    })
}

async fn load_stored_recovery(
    context: &RecoveryContext<'_>,
) -> Result<crate::journal::ExecutionPublicationRecoveryRecord, HostError> {
    match context
        .deadline
        .within(
            context
                .journal
                .execution_publication_recovery(&context.row.operation_id),
        )
        .await?
    {
        Ok(Some(stored)) => Ok(stored),
        Ok(None) | Err(HostError::Ownership) => {
            quarantine_context(context).await?;
            Err(HostError::Ownership)
        }
        Err(error) => Err(error),
    }
}

async fn decode_recovery(
    context: &RecoveryContext<'_>,
    stored: &crate::journal::ExecutionPublicationRecoveryRecord,
) -> Result<PublicationRecoveryProjection, HostError> {
    match PublicationRecoveryProjection::decode(
        &stored.recovery_projection_json,
        &stored.recovery_projection_sha256,
    ) {
        Ok(value) => Ok(value),
        Err(error) => {
            quarantine_context(context).await?;
            Err(error)
        }
    }
}

fn stored_matches(
    context: &RecoveryContext<'_>,
    stored: &crate::journal::ExecutionPublicationRecoveryRecord,
    recovery: &PublicationRecoveryProjection,
    instance_id: &str,
) -> bool {
    let row = context.row;
    stored.instance_id == instance_id
        && stored.operation_id == row.operation_id
        && stored.engine_id == row.engine_id
        && stored.docker_root_digest == row.docker_root_digest
        && stored.source_revision == row.source_revision
        && stored.runtime_image_id == row.runtime_image_id
        && stored.image_binding_digest == row.image_binding_digest
        && stored.projection_digest == row.projection_digest
        && !stored.context_digest.is_empty()
        && recovery.matches_record(stored)
        && recovery.operation_name() == row.operation_name
        && recovery.projection_digest() == row.projection_digest
        && recovery.metadata_volume_name() == stored.metadata_volume_name
        && recovery.metadata_volume_id() == stored.metadata_volume_id
}

async fn verify_metadata_volume(
    context: &RecoveryContext<'_>,
    stored: &crate::journal::ExecutionPublicationRecoveryRecord,
    name: &str,
    id: &str,
    mountpoint: &str,
    operation_id: &str,
) -> Result<(), HostError> {
    let volume = context
        .deadline
        .docker(context.docker.inspect_volume(name))
        .await?
        .map_err(|_| HostError::Docker)?;
    let labels = std::collections::HashMap::from([
        ("velnor.connection".to_owned(), stored.connection_id.clone()),
        (
            "velnor.reservation".to_owned(),
            stored.reservation_id.to_string(),
        ),
        ("velnor.operation".to_owned(), operation_id.to_owned()),
        ("velnor.role".to_owned(), "engine-metadata".to_owned()),
    ]);
    if volume.name != name
        || volume.driver != "local"
        || !volume.options.is_empty()
        || volume.labels != labels
        || volume.scope != Some(bollard::models::VolumeScopeEnum::LOCAL)
        || volume.mountpoint != mountpoint
        || id != name
    {
        quarantine_context(context).await?;
        return Err(HostError::Ownership);
    }
    Ok(())
}
