//! Shared capacity accounting and one-use admission.

mod admission;
mod enumerate;
mod lifecycle;
mod limits;
mod provisioning;
mod read;
mod storage;
mod storage_budget;

#[cfg(test)]
#[path = "reservation/storage_budget_tests.rs"]
mod storage_budget_tests;

#[cfg(test)]
#[path = "reservation/storage_budget_samples_tests.rs"]
mod storage_budget_samples_tests;

#[cfg(test)]
#[path = "reservation/storage_cache_projection_tests.rs"]
mod storage_cache_projection_tests;

pub(crate) use admission::{ReservationRequest, ReservationResult};
pub(crate) use limits::{CapacityLimits, OperationalCapacityLimits};
pub(crate) use provisioning::StorageMetadataSettlementClaim;
#[cfg(test)]
pub(crate) use provisioning::{
    ListenerDeliveryClaim, MetadataSettlementResult, RequestPhysicalReservationResult,
    StorageProvisioningPlan,
};
#[cfg(test)]
pub(crate) use read::ReservationState;
pub(crate) use storage_budget::{
    StorageAdmission, StorageFilesystemKind, StorageInodePolicy, StorageMedium, StorageObserver,
    StorageRole,
};
#[cfg(test)]
pub(crate) use storage_budget::{
    StorageAdmissionSample, StorageCacheProjection, StoragePoolLimit, StoragePoolObservation,
    StorageProfile, StorageReservationPlan, StorageRoleCharge, StorageRoleLimit,
    StorageSampleIdentity,
};

#[cfg(test)]
pub(crate) use storage_budget_tests::{
    fixture_admission, fixture_profile, fixture_provisioning_profile,
    fixture_storage_provisioning_admission,
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
