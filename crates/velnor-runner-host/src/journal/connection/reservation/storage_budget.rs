// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod job_facts_projection;
mod observations;
mod plan;
mod profile;
mod profile_store;
mod publication;
mod reservation_store;
mod role;
mod sample;

pub(crate) use job_facts_projection::{
    BoundJobFactsSnapshot, JobFactsReadContext, read_job_facts_snapshot,
};
pub(crate) use observations::{
    StoragePoolObservation, StoragePoolObservationInput, StorageSampleIdentity,
};

pub(crate) use profile::StoragePoolLimit;
pub(crate) use profile::StorageProfile;
pub(crate) use profile::StorageRoleLimit;
pub(crate) use role::{
    StorageFilesystemKind, StorageInodePolicy, StorageMedium, StorageObserver, StorageRole,
};

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
