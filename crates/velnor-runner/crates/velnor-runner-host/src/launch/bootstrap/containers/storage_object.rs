//! This module creates one claimed tmpfs volume and records its verified identity.

mod metadata;

pub(crate) use metadata::create_metadata_storage_object;

use std::collections::HashMap;
use std::future::Future;
use std::time::{Duration, Instant};

use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::models::{Volume, VolumeCreateRequest, VolumeScopeEnum};

use crate::error::HostError;
use crate::journal::{ConnectionJournal, EngineContext, StorageObjectCreateClaim, StorageRole};
use crate::launch::{SelectedEngineContext, StartObservationWitness, VerifiedWorkerImages};

use super::super::VerifiedCreatedStorageObject;

const OPERATION_TIMEOUT: Duration = Duration::from_secs(30);
const RECOVERY_TIMEOUT: Duration = Duration::from_secs(5);

/// The operation creates one claimed tmpfs volume and records its verified identity.
pub(crate) async fn create_storage_object(
    docker: &Docker,
    journal: &ConnectionJournal,
    reservation_id: i64,
    role: StorageRole,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    observation: &StartObservationWitness,
) -> Result<VerifiedCreatedStorageObject, HostError> {
    if !tmpfs_role(role) {
        return Err(HostError::Identity);
    }
    let deadline = Instant::now() + OPERATION_TIMEOUT;
    within(deadline, images.verify_for_engine(docker, engine)).await??;

    let Some(claim) = within(
        deadline,
        journal.claim_storage_object_create(reservation_id, role),
    )
    .await??
    else {
        return Err(HostError::Journal);
    };

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

const fn tmpfs_role(role: StorageRole) -> bool {
    matches!(
        role,
        StorageRole::SharedRun
            | StorageRole::RunnerHome
            | StorageRole::RunnerWork
            | StorageRole::DindData
    )
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
    if claim.engine_id() != engine.engine_id()
        || claim.docker_root_digest() != engine.docker_root_digest()
        || !engine.matches_images(images)
        || !observation.is_fresh_for_admission(
            claim.engine_id(),
            claim.docker_root_digest(),
            claim.sample_generation_id(),
        )
        || claim.medium() != crate::journal::StorageMedium::Tmpfs
        || claim.filesystem_kind() != crate::journal::StorageFilesystemKind::Tmpfs
        || claim.inode_policy() != crate::journal::StorageInodePolicy::Finite
        || claim.retained_memory_bytes() != claim.charge_bytes()
        || claim.charge_bytes() == 0
        || claim.charge_inodes() == 0
    {
        return Err(HostError::Identity);
    }
    within(deadline, images.verify_for_engine(docker, engine)).await??;
    verify_observation(claim, engine, observation)?;

    let labels = ownership_labels(journal, claim)?;
    let options = tmpfs_options(claim.charge_bytes(), claim.charge_inodes())?;
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

fn verify_observation(
    claim: &StorageObjectCreateClaim,
    engine: &SelectedEngineContext,
    observation: &StartObservationWitness,
) -> Result<(), HostError> {
    if observation.is_fresh_for_admission(
        claim.engine_id(),
        claim.docker_root_digest(),
        claim.sample_generation_id(),
    ) && claim.engine_id() == engine.engine_id()
        && claim.docker_root_digest() == engine.docker_root_digest()
    {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

async fn verify_engine(
    docker: &Docker,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
) -> Result<(), HostError> {
    let selected = EngineContext {
        engine_id: engine.engine_id().to_owned(),
        endpoint: engine.endpoint().to_owned(),
        platform: engine.platform().to_owned(),
    };
    let info = docker.info().await.map_err(|_| HostError::Docker)?;
    let current = SelectedEngineContext::capture(&selected, &info)?;
    if current.matches_images(images) {
        Ok(())
    } else {
        Err(HostError::Ownership)
    }
}

async fn inspect_volume(docker: &Docker, name: &str) -> Result<Option<Volume>, HostError> {
    match docker.inspect_volume(name).await {
        Ok(volume) => Ok(Some(volume)),
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(None),
        Err(_) => Err(HostError::Docker),
    }
}

fn ownership_labels(
    journal: &ConnectionJournal,
    claim: &StorageObjectCreateClaim,
) -> Result<HashMap<String, String>, HostError> {
    let connection_id = journal.connection_id().as_str();
    if connection_id.is_empty() || claim.docker_name().is_empty() {
        return Err(HostError::Identity);
    }
    Ok(HashMap::from([
        ("velnor.connection".to_owned(), connection_id.to_owned()),
        (
            "velnor.reservation".to_owned(),
            claim.reservation_id().to_string(),
        ),
        (
            "velnor.operation".to_owned(),
            claim.operation_id().to_string(),
        ),
        ("velnor.role".to_owned(), claim.role().as_str().to_owned()),
    ]))
}

fn tmpfs_options(bytes: u64, inodes: u64) -> Result<HashMap<String, String>, HostError> {
    if bytes == 0 || inodes == 0 {
        return Err(HostError::Identity);
    }
    Ok(HashMap::from([
        ("type".to_owned(), "tmpfs".to_owned()),
        ("device".to_owned(), "tmpfs".to_owned()),
        (
            "o".to_owned(),
            format!("size={bytes},nr_inodes={inodes},nosuid,nodev"),
        ),
    ]))
}

fn verify_volume(
    volume: &Volume,
    name: &str,
    options: &HashMap<String, String>,
    labels: &HashMap<String, String>,
) -> Result<(), HostError> {
    if volume.name != name
        || volume.driver != "local"
        || volume.options != *options
        || volume.labels != *labels
        || volume.scope != Some(VolumeScopeEnum::LOCAL)
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

async fn within<F: Future>(deadline: Instant, future: F) -> Result<F::Output, HostError> {
    tokio::time::timeout_at(deadline.into(), future)
        .await
        .map_err(|_| HostError::Docker)
}

#[cfg(test)]
#[path = "storage_object/tests.rs"]
mod tests;
