//! Shared capacity accounting and one-use admission.

mod admission;
mod enumerate;
mod lifecycle;
mod limits;
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
#[cfg(test)]
pub(crate) use read::ReservationState;
pub(crate) use storage_budget::StorageAdmission;
#[cfg(test)]
pub(crate) use storage_budget::{
    StorageAdmissionSample, StorageCacheProjection, StorageMedium, StoragePoolLimit,
    StoragePoolObservation, StorageProfile, StorageReservationPlan, StorageRole, StorageRoleCharge,
    StorageRoleLimit, StorageSampleIdentity,
};

#[cfg(test)]
pub(crate) use storage_budget_tests::{fixture_admission, fixture_profile};

pub(super) async fn validate_storage_budget_schema(
    connection: &turso::Connection,
) -> Result<(), crate::error::HostError> {
    storage_budget::validate_schema(connection).await
}
