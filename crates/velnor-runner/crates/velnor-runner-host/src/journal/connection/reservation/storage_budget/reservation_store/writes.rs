//! Durable storage records.

use crate::error::HostError;

use super::super::observations::StoragePoolObservation;
use super::super::plan::StorageReservationPlan;
use super::super::profile::StorageProfile;
use super::super::profile_store::to_sql;
use super::super::sample::{StorageAdmission, StorageAdmissionSample};
/// Persist the immutable role charge and first successful admission sample.
pub(in crate::journal::connection::reservation::storage_budget) async fn insert_reservation(
    connection: &turso::Connection,
    reservation_id: i64,
    profile: &StorageProfile,
    admission: &StorageAdmission,
) -> Result<(), HostError> {
    let retained = plan_memory(&admission.plan)?;
    connection
        .execute(
            "INSERT INTO capacity_reservation_storage (reservation_id, profile_generation_id, profile_digest, instance_binding_digest, retained_memory_bytes) VALUES (?1, ?2, ?3, ?4, ?5)",
            (reservation_id, profile.generation_id(), profile.profile_digest(), admission.plan.instance_binding_digest(), to_sql(retained)?),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    for charge in admission.plan.roles() {
        let pool = profile
            .pools()
            .iter()
            .find(|pool| pool.pool_id() == charge.pool_id())
            .ok_or(HostError::Journal)?;
        let observation = admission
            .sample
            .pools()
            .iter()
            .find(|pool| pool.pool_id() == charge.pool_id())
            .ok_or(HostError::Journal)?;
        connection
            .execute(
                "INSERT INTO capacity_reservation_storage_roles (reservation_id, role, pool_id, filesystem_instance_digest, medium, inode_policy, filesystem_kind, charge_bytes, charge_inodes, retained_memory_bytes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                (reservation_id, charge.role().as_str(), charge.pool_id(), charge.filesystem_instance_digest(), charge.medium().as_str(), pool.inode_policy().as_str(), observation.filesystem_kind().as_str(), to_sql(charge.charge_bytes())?, to_sql(charge.charge_inodes())?, to_sql(charge.retained_memory_bytes())?),
            )
            .await
            .map_err(|_| HostError::Journal)?;
    }
    insert_sample(connection, reservation_id, profile, admission.sample()).await
}

/// Append a new exact sample while retaining the original reservation plan.
pub(in crate::journal::connection::reservation::storage_budget) async fn append_sample(
    connection: &turso::Connection,
    reservation_id: i64,
    profile: &StorageProfile,
    sample: &StorageAdmissionSample,
) -> Result<(), HostError> {
    insert_sample(connection, reservation_id, profile, sample).await
}

async fn insert_sample(
    connection: &turso::Connection,
    reservation_id: i64,
    profile: &StorageProfile,
    sample: &StorageAdmissionSample,
) -> Result<(), HostError> {
    let identity = sample.identity();
    let memory = sample.memory();
    connection
        .execute(
            "INSERT INTO capacity_reservation_storage_samples (reservation_id, sample_generation_id, profile_generation_id, profile_digest, engine_id, docker_root_digest, observed_at_unix_ms, guest_available_bytes, ancestor_memory_max_bytes, ancestor_memory_current_bytes, ancestor_swap_max_bytes, ancestor_swap_current_bytes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            (reservation_id, identity.sample_generation_id(), identity.profile_generation_id(), identity.profile_digest(), identity.engine_id(), identity.docker_root_digest(), to_sql(sample.observed_at_unix_ms())?, memory.map(|value| to_sql(value.guest_available_bytes())).transpose()?, memory.map(|value| to_sql(value.ancestor_memory_max_bytes())).transpose()?, memory.map(|value| to_sql(value.ancestor_memory_current_bytes())).transpose()?, memory.map(|value| to_sql(value.ancestor_swap_max_bytes())).transpose()?, memory.map(|value| to_sql(value.ancestor_swap_current_bytes())).transpose()?),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    for pool_limit in profile.pools() {
        let pool: &StoragePoolObservation = sample
            .pools()
            .iter()
            .find(|pool| pool.pool_id() == pool_limit.pool_id())
            .ok_or(HostError::Journal)?;
        connection
            .execute(
                "INSERT INTO capacity_reservation_storage_pools (reservation_id, sample_generation_id, pool_id, filesystem_instance_digest, medium, filesystem_kind, inode_policy, total_bytes, available_bytes, total_inodes, available_inodes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                (reservation_id, identity.sample_generation_id(), pool.pool_id(), pool.filesystem_instance_digest(), pool.medium().as_str(), pool.filesystem_kind().as_str(), pool_limit.inode_policy().as_str(), to_sql(pool.total_bytes())?, to_sql(pool.available_bytes())?, to_sql(pool.total_inodes())?, to_sql(pool.available_inodes())?),
            )
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

pub(in crate::journal::connection::reservation::storage_budget) fn plan_memory(
    plan: &StorageReservationPlan,
) -> Result<u64, HostError> {
    plan.roles().iter().try_fold(0_u64, |sum, role| {
        sum.checked_add(role.retained_memory_bytes())
            .ok_or(HostError::Journal)
    })
}
