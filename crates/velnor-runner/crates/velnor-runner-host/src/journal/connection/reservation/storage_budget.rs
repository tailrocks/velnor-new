//! Typed, bounded storage charges carried with one host reservation.

mod job_facts_projection;
mod observations;
mod plan;
mod profile;
mod profile_store;
mod publication;
mod reservation_store;
mod role;
mod sample;
#[cfg(test)]
pub(crate) use observations::StorageMemoryObservation;

pub(crate) use job_facts_projection::{
    BoundJobFactsSnapshot, JobFactsReadContext, read_job_facts_snapshot,
};
pub(crate) use observations::{StoragePoolObservation, StorageSampleIdentity};
#[cfg(test)]
pub(crate) use plan::{StorageReservationPlan, StorageRoleCharge};
pub(crate) use profile::StoragePoolLimit;
pub(crate) use profile::StorageProfile;
pub(crate) use profile::StorageRoleLimit;
pub(crate) use role::{
    StorageFilesystemKind, StorageInodePolicy, StorageMedium, StorageObserver, StorageRole,
};
#[cfg(test)]
pub(crate) use sample::StorageAdmission;
pub(crate) use sample::{
    StorageAdmissionSample, StorageProvisioningAdmission, StorageProvisioningMemoryObservation,
};

pub(super) async fn validate_schema(
    conn: &turso::Connection,
) -> Result<(), crate::error::HostError> {
    profile_store::validate_publication_policies(conn).await?;
    super::storage_budget::reservation_store::validate_rows(conn).await
}

pub(super) async fn current_profile(
    conn: &turso::Connection,
) -> Result<Option<StorageProfile>, crate::error::HostError> {
    profile_store::current(conn).await
}

pub(super) async fn profile_generation(
    conn: &turso::Connection,
    generation_id: &str,
) -> Result<Option<StorageProfile>, crate::error::HostError> {
    profile_store::load(conn, generation_id).await
}

#[cfg(test)]
pub(super) async fn admission_fits(
    conn: &turso::Connection,
    admission: &StorageAdmission,
    sample_generation_id: &str,
    engine_id: &str,
    docker_root_digest: &str,
    base_memory_charge: u64,
    excluded_reservation_id: Option<i64>,
) -> Result<bool, crate::error::HostError> {
    reservation_store::admission_fits(
        conn,
        admission,
        sample_generation_id,
        engine_id,
        docker_root_digest,
        base_memory_charge,
        excluded_reservation_id,
    )
    .await
}

#[cfg(test)]
pub(super) async fn admission_matches_existing(
    conn: &turso::Connection,
    reservation_id: i64,
    admission: &StorageAdmission,
) -> Result<bool, crate::error::HostError> {
    reservation_store::matches_existing(conn, reservation_id, admission).await
}

#[cfg(test)]
pub(super) async fn insert_reservation_storage(
    conn: &turso::Connection,
    reservation_id: i64,
    profile: &StorageProfile,
    admission: &StorageAdmission,
) -> Result<(), crate::error::HostError> {
    reservation_store::insert_reservation(conn, reservation_id, profile, admission).await
}

#[cfg(test)]
pub(super) async fn append_storage_sample(
    conn: &turso::Connection,
    reservation_id: i64,
    profile: &StorageProfile,
    sample: &StorageAdmissionSample,
) -> Result<(), crate::error::HostError> {
    reservation_store::append_sample(conn, reservation_id, profile, sample).await
}

pub(super) const fn valid_generation(value: &str) -> bool {
    valid_lower_hex(value, 32)
}

pub(super) const fn valid_digest(value: &str) -> bool {
    valid_lower_hex(value, 64)
}

const fn valid_lower_hex(value: &str, length: usize) -> bool {
    if value.len() != length {
        return false;
    }
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if !(byte.is_ascii_digit() || (byte >= b'a' && byte <= b'f')) {
            return false;
        }
        index += 1;
    }
    true
}
