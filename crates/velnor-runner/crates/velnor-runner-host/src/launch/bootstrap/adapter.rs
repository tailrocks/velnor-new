use bollard::Docker;

use crate::error::HostError;
use crate::journal::ConnectionJournal;
use crate::launch::steps::{AcquiredProvisioning, ScopedProvisioningOutcome};
use crate::launch::{SelectedEngineContext, VerifiedWorkerImages};

use super::containers::VerifiedCreatedStorageObject;
use super::storage::{self, PreparedTmpfsStorage};

/// One acquired request with its initialized storage objects.
#[must_use]
pub(crate) struct PreparedAcquiredBootstrap {
    acquired: AcquiredProvisioning,
    storage: PreparedTmpfsStorage,
    metadata: VerifiedCreatedStorageObject,
}

impl PreparedAcquiredBootstrap {
    pub(crate) fn acquired(&self) -> &AcquiredProvisioning {
        &self.acquired
    }

    pub(crate) fn storage(&self) -> &PreparedTmpfsStorage {
        &self.storage
    }

    pub(crate) fn storage_mut(&mut self) -> &mut PreparedTmpfsStorage {
        &mut self.storage
    }

    pub(crate) fn metadata(&self) -> &VerifiedCreatedStorageObject {
        &self.metadata
    }
}

/// Create and initialize storage for one durably acquired request.
pub(crate) async fn prepare_acquired(
    outcome: ScopedProvisioningOutcome,
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
) -> Result<Option<PreparedAcquiredBootstrap>, HostError> {
    let acquired = match outcome {
        ScopedProvisioningOutcome::NoStart => return Ok(None),
        ScopedProvisioningOutcome::Acquired(acquired) => acquired,
        ScopedProvisioningOutcome::Recovered(recovery) => recovery.into_acquired(),
    };
    if acquired.reservation_id() <= 0
        || !engine.matches_images(images)
        || !acquired.observation().is_fresh_for_admission(
            engine.engine_id(),
            engine.docker_root_digest(),
            acquired.sample_generation_id(),
        )
    {
        return Err(HostError::Identity);
    }
    let metadata = super::containers::create_metadata_storage_object(
        docker,
        journal,
        acquired.reservation_id(),
        engine,
        images,
        acquired.observation(),
    )
    .await?;
    if metadata.reservation_id() != acquired.reservation_id()
        || metadata.operation_id() != acquired.intent_id()
        || metadata.engine_id() != engine.engine_id()
        || metadata.docker_root_digest() != engine.docker_root_digest()
        || metadata.role() != crate::journal::StorageRole::EngineMetadata
    {
        return Err(HostError::Identity);
    }
    let storage = storage::create_and_initialize(
        docker,
        journal,
        acquired.reservation_id(),
        acquired.intent_id(),
        engine,
        images,
        acquired.observation(),
        acquired.resource_budget(),
        &metadata,
    )
    .await?;
    Ok(Some(PreparedAcquiredBootstrap {
        acquired,
        storage,
        metadata,
    }))
}
