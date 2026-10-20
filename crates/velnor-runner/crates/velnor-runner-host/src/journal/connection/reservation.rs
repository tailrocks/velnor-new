//! Shared capacity accounting and one-use admission.

mod admission;
mod enumerate;
mod lifecycle;
mod limits;
mod physical_attempt;
mod provisioning;

pub(in crate::journal) use provisioning::validate_retirement_digest;

pub(in crate::journal) async fn release_ready(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<bool, crate::error::HostError> {
    provisioning::release_ready(connection, reservation_id).await
}

pub(in crate::journal) async fn record_no_acquire_outcome(
    connection: &turso::Connection,
    reservation_id: i64,
    response_digest: &str,
) -> Result<(), crate::error::HostError> {
    physical_attempt::record_no_acquire_outcome(connection, reservation_id, response_digest).await
}

pub(in crate::journal) async fn record_terminal_cleanup_outcome(
    connection: &turso::Connection,
    connection_id: &str,
    reservation_id: i64,
    intent_id: i64,
) -> Result<bool, crate::error::HostError> {
    physical_attempt::record_terminal_cleanup_outcome(
        connection,
        connection_id,
        reservation_id,
        intent_id,
    )
    .await
}
mod read;
mod storage;
mod storage_budget;

pub(crate) use enumerate::{
    PhysicalRunnerJitState, PhysicalRunnerOwnership, PhysicalRunnerReservation,
};
#[cfg(test)]
pub(crate) use physical_attempt::PhysicalAttemptIdentity;
#[cfg(test)]
pub(crate) use read::ReservationSnapshot;
pub(crate) use read::ReservationState;

#[cfg(test)]
#[path = "reservation/storage_budget_tests.rs"]
mod storage_budget_tests;

#[cfg(test)]
#[path = "reservation/storage_budget_samples_tests.rs"]
mod storage_budget_samples_tests;

#[cfg(test)]
#[path = "reservation/network_tests.rs"]
mod network_tests;

pub(crate) use admission::ReservationRequest;
#[cfg(test)]
pub(crate) use admission::ReservationResult;
pub(crate) use limits::{CapacityLimits, OperationalCapacityLimits};
pub(crate) use provisioning::JobFactsPublicationClaim;
pub(crate) use provisioning::ListenerDeliveryClaim;
pub(crate) use provisioning::MetadataSettlementResult;
pub(crate) use provisioning::PreJitCapacityProbeClaim;
pub(crate) use provisioning::PreJitCleanupClaim;
pub(crate) use provisioning::PreJitCleanupSettlementResult;
pub(crate) use provisioning::StorageCleanupObserverWitness;
pub(crate) use provisioning::{
    BootstrapContainerCreateClaim, BootstrapStartClaim, RecordedStorageNetwork,
};
#[cfg(test)]
pub(crate) use provisioning::{CreateCommitStage, arm_create_commit_gate};
pub(crate) use provisioning::{
    CreateInterruptionOutcome, PhysicalSuccessorReservationResult,
    RequestPhysicalReservationResult, StorageCleanupIdentity, StorageInitializationClaim,
    StorageMetadataSettlementClaim, StorageNetworkCreateClaim, StorageNetworkRemovalClaim,
    StorageObjectCreateClaim, StorageProvisioningPlan, StorageProvisioningRole,
    VerifiedMetadataInitialization, VerifiedMetadataInitializationHandoff,
};
pub(crate) use provisioning::{
    PreJitBootstrapContainer, PreJitCleanupInventory, PreJitCreateSpec, PreJitInitializerIdentity,
    PreJitNetworkIdentity, PreJitStorageObject,
};
#[cfg(test)]
pub(crate) use provisioning::{
    StorageCleanupCgroup, StorageCleanupMount, StorageNetworkRecoveryIdentity,
};
#[cfg(test)]
pub(crate) use storage_budget::StorageAdmission;
pub(crate) use storage_budget::{
    BoundJobFactsSnapshot, StorageAdmissionSample, StorageFilesystemKind, StorageInodePolicy,
    StorageMedium, StorageObserver, StoragePoolLimit, StoragePoolObservation, StorageProfile,
    StorageProvisioningAdmission, StorageProvisioningMemoryObservation, StorageRole,
    StorageRoleLimit, StorageSampleIdentity,
};
#[cfg(test)]
pub(crate) use storage_budget::{StorageReservationPlan, StorageRoleCharge};

#[cfg(test)]
pub(crate) use storage_budget_tests::{
    fixture_admission, fixture_profile, fixture_provisioning_profile,
    fixture_storage_provisioning_admission, fixture_storage_provisioning_admission_for_profile,
    fixture_storage_provisioning_admission_with_filesystem_digest,
};

pub(super) async fn validate_storage_budget_schema(
    connection: &turso::Connection,
) -> Result<(), crate::error::HostError> {
    storage_budget::validate_schema(connection).await
}

pub(in crate::journal) async fn insert_metadata_settlement_claim(
    connection: &turso::Connection,
    scope: &super::ConnectionJournal,
    reservation_id: i64,
    seed: &crate::journal::resource_probe::ProbeSeed,
) -> Result<StorageMetadataSettlementClaim, crate::error::HostError> {
    provisioning::insert_metadata_settlement_claim(connection, scope, reservation_id, seed).await
}

pub(in crate::journal) async fn record_metadata_probe_cleanup(
    connection: &turso::Connection,
    operation_id: &str,
    engine_id: &str,
    docker_root_digest: &str,
    operation_name: &str,
    container_id: &str,
) -> Result<(), crate::error::HostError> {
    provisioning::record_metadata_probe_cleanup(
        connection,
        operation_id,
        engine_id,
        docker_root_digest,
        operation_name,
        container_id,
    )
    .await
}

pub(in crate::journal) async fn resources_bound(
    connection: &turso::Connection,
    connection_id: &str,
    intent_id: i64,
) -> Result<bool, crate::error::HostError> {
    provisioning::resources_bound(connection, connection_id, intent_id).await
}
