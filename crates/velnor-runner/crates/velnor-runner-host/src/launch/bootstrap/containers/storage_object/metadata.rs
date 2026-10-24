use std::collections::HashMap;
use std::time::Instant;

use bollard::Docker;
use bollard::models::VolumeCreateRequest;

use crate::error::HostError;
use crate::journal::{ConnectionJournal, StorageObjectCreateClaim, StorageRole};
use crate::launch::{SelectedEngineContext, StartObservationWitness, VerifiedWorkerImages};

use super::{
    OPERATION_TIMEOUT, RECOVERY_TIMEOUT, VerifiedCreatedStorageObject, inspect_volume,
    ownership_labels, verify_engine, verify_observation, verify_volume, within,
};

/// Create the metadata volume after the request is acquired.
pub(crate) async fn create_metadata_storage_object(
    docker: &Docker,
    journal: &ConnectionJournal,
    reservation_id: i64,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    observation: &StartObservationWitness,
) -> Result<VerifiedCreatedStorageObject, HostError> {
    if reservation_id <= 0 {
        return Err(HostError::Identity);
    }
    let deadline = Instant::now() + OPERATION_TIMEOUT;
    within(deadline, images.verify_for_engine(docker, engine)).await??;
    let claim = within(
        deadline,
        journal.claim_storage_object_create(reservation_id, StorageRole::EngineMetadata),
    )
    .await??
    .ok_or(HostError::Journal)?;
    let result = create_claimed(
        docker,
        journal,
        &claim,
        engine,
        images,
        observation,
        deadline,
    )
    .await;
    if result.is_err() {
        let _marked = tokio::time::timeout(
            RECOVERY_TIMEOUT,
            journal.mark_storage_object_create_uncertain(&claim),
        )
        .await;
    }
    result
}

async fn create_claimed(
    docker: &Docker,
    journal: &ConnectionJournal,
    claim: &StorageObjectCreateClaim,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    observation: &StartObservationWitness,
    deadline: Instant,
) -> Result<VerifiedCreatedStorageObject, HostError> {
    if claim.role() != StorageRole::EngineMetadata
        || claim.engine_id() != engine.engine_id()
        || claim.docker_root_digest() != engine.docker_root_digest()
        || claim.medium() != crate::journal::StorageMedium::Disk
        || claim.filesystem_kind() != crate::journal::StorageFilesystemKind::Btrfs
        || claim.inode_policy() != crate::journal::StorageInodePolicy::DynamicBtrfs
        || claim.charge_bytes() == 0
        || claim.retained_memory_bytes() != 0
        || !engine.matches_images(images)
    {
        return Err(HostError::Identity);
    }
    within(deadline, images.verify_for_engine(docker, engine)).await??;
    verify_observation(claim, engine, observation)?;
    let labels = ownership_labels(journal, claim)?;
    let options = HashMap::new();
    if within(deadline, inspect_volume(docker, claim.docker_name()))
        .await??
        .is_some()
    {
        return Err(HostError::Ownership);
    }
    within(deadline, verify_engine(docker, engine, images)).await??;
    verify_observation(claim, engine, observation)?;
    let created = within(
        deadline,
        docker.create_volume(VolumeCreateRequest {
            name: Some(claim.docker_name().to_owned()),
            driver: Some("local".to_owned()),
            driver_opts: Some(options.clone()),
            labels: Some(labels.clone()),
            ..Default::default()
        }),
    )
    .await?
    .map_err(|_| HostError::Docker)?;
    within(deadline, verify_engine(docker, engine, images)).await??;
    verify_volume(&created, claim.docker_name(), &options, &labels)?;
    let inspected = within(deadline, inspect_volume(docker, claim.docker_name()))
        .await??
        .ok_or(HostError::Docker)?;
    within(deadline, verify_engine(docker, engine, images)).await??;
    verify_volume(&inspected, claim.docker_name(), &options, &labels)?;
    let proof = VerifiedCreatedStorageObject {
        reservation_id: claim.reservation_id(),
        operation_id: claim.operation_id(),
        engine_id: claim.engine_id().to_owned(),
        docker_root_digest: claim.docker_root_digest().to_owned(),
        role: claim.role(),
        docker_name: claim.docker_name().to_owned(),
        docker_id: inspected.name,
    };
    within(
        deadline,
        journal.record_storage_object_created(claim, &proof),
    )
    .await??;
    Ok(proof)
}
