use std::time::Duration;

use crate::error::HostError;
use crate::journal::StorageRole;
use crate::worker::ResourceBudget;
use crate::worker::storage_initialization::{
    ObservedStorageInitialization, PendingStorageInitialization, StorageInitializationImage,
    StorageInitializationLimits,
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

/// Storage objects created before job containers.
pub(crate) struct PreparedTmpfsStorage {
    objects: [VerifiedCreatedStorageObject; 4],
    pending_initialization: Option<PendingStorageInitialization>,
    initialized: ObservedStorageInitialization,
}

impl PreparedTmpfsStorage {
    pub(crate) fn objects(&self) -> &[VerifiedCreatedStorageObject; 4] {
        &self.objects
    }

    pub(crate) fn initialized(&self) -> &ObservedStorageInitialization {
        &self.initialized
    }

    pub(crate) fn pending_initialization(&self) -> Option<&PendingStorageInitialization> {
        self.pending_initialization.as_ref()
    }

    pub(crate) fn pending_initialization_mut(
        &mut self,
    ) -> &mut Option<PendingStorageInitialization> {
        &mut self.pending_initialization
    }

    pub(crate) fn take_pending_initialization(&mut self) -> Option<PendingStorageInitialization> {
        self.pending_initialization.take()
    }
}

/// Create each claimed tmpfs object and initialize Home and Work.
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
    let initialized = pending_initialization.observed().clone();
    Ok(PreparedTmpfsStorage {
        objects,
        pending_initialization: Some(pending_initialization),
        initialized,
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

#[cfg(test)]
#[path = "production/tests.rs"]
mod tests;

#[cfg(test)]
mod metadata_identity_tests {
    use super::{TMPFS_ROLES, validate_metadata_identity, validate_tmpfs_objects};
    use crate::error::HostError;
    use crate::journal::StorageRole;
    use crate::launch::bootstrap::containers::test_created_storage_object;

    const RESERVATION_ID: i64 = 7;
    const OPERATION_ID: i64 = 11;
    const ENGINE_ID: &str = "11111111-1111-4111-8111-111111111111";
    const ROOT_DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn metadata(
        reservation_id: i64,
        operation_id: i64,
        engine_id: &str,
        root_digest: &str,
        role: StorageRole,
    ) -> crate::launch::bootstrap::VerifiedCreatedStorageObject {
        let name = test_volume_name(role);
        test_created_storage_object(
            reservation_id,
            operation_id,
            engine_id.to_owned(),
            root_digest.to_owned(),
            role,
            name.clone(),
            name,
        )
        .expect("test metadata identity is valid")
    }

    fn test_volume_name(role: StorageRole) -> String {
        let suffix = match role {
            StorageRole::SharedRun => "",
            StorageRole::RunnerHome => "-home",
            StorageRole::RunnerWork => "-work",
            StorageRole::DindData => "-docker",
            StorageRole::EngineMetadata => "-metadata",
            _ => "-invalid",
        };
        format!("w{}{}", "a".repeat(32), suffix)
    }

    #[test]
    fn metadata_identity_must_match_before_storage_initialization() {
        let valid = metadata(
            RESERVATION_ID,
            OPERATION_ID,
            ENGINE_ID,
            ROOT_DIGEST,
            StorageRole::EngineMetadata,
        );
        assert!(
            validate_metadata_identity(
                &valid,
                RESERVATION_ID,
                OPERATION_ID,
                ENGINE_ID,
                ROOT_DIGEST,
            )
            .is_ok()
        );

        let mismatches = [
            metadata(
                RESERVATION_ID + 1,
                OPERATION_ID,
                ENGINE_ID,
                ROOT_DIGEST,
                StorageRole::EngineMetadata,
            ),
            metadata(
                RESERVATION_ID,
                OPERATION_ID + 1,
                ENGINE_ID,
                ROOT_DIGEST,
                StorageRole::EngineMetadata,
            ),
            metadata(
                RESERVATION_ID,
                OPERATION_ID,
                "22222222-2222-4222-8222-222222222222",
                ROOT_DIGEST,
                StorageRole::EngineMetadata,
            ),
            metadata(
                RESERVATION_ID,
                OPERATION_ID,
                ENGINE_ID,
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                StorageRole::EngineMetadata,
            ),
            metadata(
                RESERVATION_ID,
                OPERATION_ID,
                ENGINE_ID,
                ROOT_DIGEST,
                StorageRole::RunnerHome,
            ),
        ];
        for metadata in mismatches {
            assert!(matches!(
                validate_metadata_identity(
                    &metadata,
                    RESERVATION_ID,
                    OPERATION_ID,
                    ENGINE_ID,
                    ROOT_DIGEST,
                ),
                Err(HostError::Identity)
            ));
        }
        assert!(matches!(
            validate_metadata_identity(&valid, 0, OPERATION_ID, ENGINE_ID, ROOT_DIGEST),
            Err(HostError::Identity)
        ));
    }

    #[test]
    fn tmpfs_objects_must_match_validated_metadata_before_initializer_start() {
        let metadata = metadata(
            RESERVATION_ID,
            OPERATION_ID,
            ENGINE_ID,
            ROOT_DIGEST,
            StorageRole::EngineMetadata,
        );
        let objects = TMPFS_ROLES.map(|role| {
            test_created_storage_object(
                RESERVATION_ID,
                OPERATION_ID,
                ENGINE_ID.to_owned(),
                ROOT_DIGEST.to_owned(),
                role,
                test_volume_name(role),
                test_volume_name(role),
            )
            .expect("test tmpfs identity is valid")
        });
        assert!(validate_tmpfs_objects(&objects, &metadata).is_ok());

        let mut wrong_operation = objects;
        wrong_operation[1] = test_created_storage_object(
            RESERVATION_ID,
            OPERATION_ID + 1,
            ENGINE_ID.to_owned(),
            ROOT_DIGEST.to_owned(),
            TMPFS_ROLES[1],
            test_volume_name(TMPFS_ROLES[1]),
            test_volume_name(TMPFS_ROLES[1]),
        )
        .expect("mismatched operation remains a valid object receipt");
        assert!(matches!(
            validate_tmpfs_objects(&wrong_operation, &metadata),
            Err(HostError::Identity)
        ));
    }
}
