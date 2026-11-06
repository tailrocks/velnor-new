// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use crate::error::HostError;
use crate::journal::StorageRole;
use crate::worker::ResourceBudget;
use crate::worker::storage_initialization::{
    PendingStorageInitialization, StorageInitializationImage, StorageInitializationLimits,
};

use super::super::adapter::StorageCreationContext;
use super::super::containers::{VerifiedCreatedStorageObject, create_storage_object};

const INITIALIZER_PIDS: u64 = 16;
const INITIALIZER_TIMEOUT: Duration = Duration::from_secs(30);
const TMPFS_ROLES: [StorageRole; 4] = [
    StorageRole::SharedRun,
    StorageRole::RunnerHome,
    StorageRole::RunnerWork,
    StorageRole::DindData,
];

pub(crate) struct PreparedTmpfsStorage {
    objects: [VerifiedCreatedStorageObject; 4],
    pending_initialization: Option<PendingStorageInitialization>,
}

impl PreparedTmpfsStorage {
    pub(crate) fn objects(&self) -> &[VerifiedCreatedStorageObject; 4] {
        &self.objects
    }

    pub(crate) fn pending_initialization(&self) -> Option<&PendingStorageInitialization> {
        self.pending_initialization.as_ref()
    }

    pub(crate) fn pending_initialization_mut(
        &mut self,
    ) -> &mut Option<PendingStorageInitialization> {
        &mut self.pending_initialization
    }
}

pub(crate) async fn create_and_initialize(
    context: &StorageCreationContext<'_>,
    reservation_id: i64,
    operation_id: i64,
    budget: ResourceBudget,
) -> Result<PreparedTmpfsStorage, HostError> {
    let StorageCreationContext {
        docker,
        journal,
        engine,
        images,
        observation,
        metadata,
    } = context;
    validate_metadata_identity(
        metadata,
        reservation_id,
        operation_id,
        engine.engine_id(),
        engine.docker_root_digest(),
    )?;
    let mut created = Vec::with_capacity(TMPFS_ROLES.len());
    for role in TMPFS_ROLES {
        created.push(
            create_storage_object(
                docker,
                journal,
                reservation_id,
                role,
                engine,
                images,
                observation,
            )
            .await?,
        );
    }
    let objects: [VerifiedCreatedStorageObject; 4] =
        created.try_into().map_err(|_| HostError::Journal)?;
    validate_tmpfs_objects(&objects, metadata)?;
    let limits = initialization_limits(budget)?;
    let image = StorageInitializationImage::from_verified_release(images)?;
    let pending_initialization = crate::worker::storage_initialization::initialize(
        docker,
        journal,
        reservation_id,
        &image,
        limits,
        INITIALIZER_TIMEOUT,
        metadata,
    )
    .await?;
    Ok(PreparedTmpfsStorage {
        objects,
        pending_initialization: Some(pending_initialization),
    })
}

fn validate_metadata_identity(
    metadata: &VerifiedCreatedStorageObject,
    reservation_id: i64,
    operation_id: i64,
    engine_id: &str,
    docker_root_digest: &str,
) -> Result<(), HostError> {
    if reservation_id <= 0
        || operation_id <= 0
        || metadata.reservation_id() != reservation_id
        || metadata.operation_id() != operation_id
        || metadata.engine_id() != engine_id
        || metadata.docker_root_digest() != docker_root_digest
        || metadata.role() != StorageRole::EngineMetadata
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn validate_tmpfs_objects(
    objects: &[VerifiedCreatedStorageObject; 4],
    metadata: &VerifiedCreatedStorageObject,
) -> Result<(), HostError> {
    for (object, expected_role) in objects.iter().zip(TMPFS_ROLES) {
        if object.reservation_id() != metadata.reservation_id()
            || object.operation_id() != metadata.operation_id()
            || object.engine_id() != metadata.engine_id()
            || object.docker_root_digest() != metadata.docker_root_digest()
            || object.role() != expected_role
        {
            return Err(HostError::Identity);
        }
    }
    Ok(())
}

fn initialization_limits(budget: ResourceBudget) -> Result<StorageInitializationLimits, HostError> {
    let runner = budget.runner();
    let reserve = budget.admission_reserve();
    let runner_cpu = u64::try_from(runner.nano_cpus).map_err(|_| HostError::Config)? / 1_000_000;
    let runner_memory = u64::try_from(runner.memory_bytes).map_err(|_| HostError::Config)?;
    StorageInitializationLimits::from_reserved_budget(
        budget,
        runner_cpu.min(reserve.cpu_millicores),
        runner_memory.min(reserve.memory_bytes),
        INITIALIZER_PIDS,
    )
}
