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
pub(crate) use enumerate::{
    PhysicalRunnerJitState, PhysicalRunnerOwnership, PhysicalRunnerReservation,
};
pub(crate) use limits::{CapacityLimits, OperationalCapacityLimits};
pub(crate) use provisioning::{
    BootstrapContainerCreateClaim, BootstrapStartClaim, JobFactsPublicationClaim,
    ListenerDeliveryClaim, StorageObjectCreateClaim,
};
pub(crate) use read::{ReservationSnapshot, ReservationState};
pub(crate) use storage_budget::{
    StorageAdmission, StorageAdmissionSample, StorageCacheProjection,
    StorageAttachmentContract, StorageFilesystemKind, StorageInodePolicy, StorageMedium,
    StorageMemoryObservation, StorageObserver, StorageProvisioningAdmission, StoragePoolLimit,
    StoragePoolObservation, StorageProfile, StorageProvisioningMemoryObservation,
    StorageReservationPlan, StorageRole, StorageRoleCharge, StorageRoleContract, StorageRoleLimit,
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
