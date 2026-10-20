//! Stored capacity sample validation.

use std::collections::BTreeMap;

use crate::error::HostError;

use super::super::plan::StorageReservationPlan;
use super::super::profile::StorageProfile;
use super::super::profile_store::from_sql;
use super::super::role::{StorageFilesystemKind, StorageInodePolicy, StorageMedium};
use super::super::{valid_digest, valid_generation};

pub(in crate::journal::connection::reservation::storage_budget) async fn validate_sample_history(
    connection: &turso::Connection,
    reservation_id: i64,
    profile: &StorageProfile,
    plan: &StorageReservationPlan,
    expected_engine_id: &str,
    expected_root_digest: &str,
    role_kinds: &BTreeMap<String, StorageFilesystemKind>,
) -> Result<(), HostError> {
    let mut samples = connection
        .query(
            "SELECT sample_generation_id, profile_generation_id, profile_digest, engine_id, docker_root_digest, observed_at_unix_ms, guest_available_bytes, ancestor_memory_max_bytes, ancestor_memory_current_bytes, ancestor_swap_max_bytes, ancestor_swap_current_bytes FROM capacity_reservation_storage_samples WHERE reservation_id = ?1 ORDER BY sample_generation_id",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut found = false;
    while let Some(sample) = samples.next().await.map_err(|_| HostError::Journal)? {
        found = true;
        let sample_id: String = sample.get(0).map_err(|_| HostError::Journal)?;
        let generation: String = sample.get(1).map_err(|_| HostError::Journal)?;
        let profile_digest: String = sample.get(2).map_err(|_| HostError::Journal)?;
        let engine_id: String = sample.get(3).map_err(|_| HostError::Journal)?;
        let root_digest: String = sample.get(4).map_err(|_| HostError::Journal)?;
        let observed_at: i64 = sample.get(5).map_err(|_| HostError::Journal)?;
        if !valid_generation(&sample_id)
            || generation != profile.generation_id()
            || profile_digest != profile.profile_digest()
            || engine_id != expected_engine_id
            || root_digest != expected_root_digest
            || observed_at <= 0
            || read_capacity_sample(connection, reservation_id, &sample_id).await? != Some(())
        {
            return Err(HostError::Journal);
        }
        let has_memory = validate_sample_memory_columns(&sample)?;
        if profile
            .pools()
            .iter()
            .any(|pool| pool.medium() == StorageMedium::Tmpfs)
            && !has_memory
        {
            return Err(HostError::Journal);
        }
        validate_sample_pools(
            connection,
            reservation_id,
            &sample_id,
            profile,
            plan,
            role_kinds,
        )
        .await?;
    }
    if !found {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn read_capacity_sample(
    connection: &turso::Connection,
    reservation_id: i64,
    sample_id: &str,
) -> Result<Option<()>, HostError> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM capacity_reservation_samples WHERE reservation_id = ?1 AND sample_generation_id = ?2",
            (reservation_id, sample_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .map(|_| ()))
}

fn validate_sample_memory_columns(row: &turso::Row) -> Result<bool, HostError> {
    let guest: Option<i64> = row.get(6).map_err(|_| HostError::Journal)?;
    let memory_max: Option<i64> = row.get(7).map_err(|_| HostError::Journal)?;
    let memory_current: Option<i64> = row.get(8).map_err(|_| HostError::Journal)?;
    let swap_max: Option<i64> = row.get(9).map_err(|_| HostError::Journal)?;
    let swap_current: Option<i64> = row.get(10).map_err(|_| HostError::Journal)?;
    match (guest, memory_max, memory_current, swap_max, swap_current) {
        (None, None, None, None, None) => Ok(false),
        (Some(guest), Some(max), Some(current), Some(swap_max), Some(swap_current))
            if guest > 0
                && max > 0
                && current >= 0
                && current <= max
                && swap_max >= 0
                && swap_current >= 0
                && swap_current <= swap_max =>
        {
            Ok(true)
        }
        _ => Err(HostError::Journal),
    }
}

async fn validate_sample_pools(
    connection: &turso::Connection,
    reservation_id: i64,
    sample_id: &str,
    profile: &StorageProfile,
    plan: &StorageReservationPlan,
    role_kinds: &BTreeMap<String, StorageFilesystemKind>,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT pool_id, filesystem_instance_digest, medium, filesystem_kind, inode_policy, total_bytes, available_bytes, total_inodes, available_inodes FROM capacity_reservation_storage_pools WHERE reservation_id = ?1 AND sample_generation_id = ?2 ORDER BY pool_id",
            (reservation_id, sample_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut count = 0;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        count += 1;
        let pool_id: String = row.get(0).map_err(|_| HostError::Journal)?;
        let fs_digest: String = row.get(1).map_err(|_| HostError::Journal)?;
        let medium = StorageMedium::parse(&row.get::<String>(2).map_err(|_| HostError::Journal)?)?;
        let fs_kind =
            StorageFilesystemKind::parse(&row.get::<String>(3).map_err(|_| HostError::Journal)?)?;
        let inode_policy =
            StorageInodePolicy::parse(&row.get::<String>(4).map_err(|_| HostError::Journal)?)?;
        let total_bytes = from_sql(row.get(5).map_err(|_| HostError::Journal)?)?;
        let available_bytes = from_sql(row.get(6).map_err(|_| HostError::Journal)?)?;
        let total_inodes = from_sql(row.get(7).map_err(|_| HostError::Journal)?)?;
        let available_inodes = from_sql(row.get(8).map_err(|_| HostError::Journal)?)?;
        let limit = profile
            .pools()
            .iter()
            .find(|limit| limit.pool_id() == pool_id)
            .ok_or(HostError::Journal)?;
        if !valid_digest(&fs_digest)
            || medium != limit.medium()
            || inode_policy != limit.inode_policy()
            || role_kinds.get(&pool_id) != Some(&fs_kind)
            || total_bytes < limit.max_bytes()
            || available_bytes > total_bytes
            || available_inodes > total_inodes
            || inode_policy == StorageInodePolicy::Unknown
            || (inode_policy == StorageInodePolicy::DynamicBtrfs
                && (fs_kind != StorageFilesystemKind::Btrfs
                    || total_inodes != 0
                    || available_inodes != 0))
            || (inode_policy == StorageInodePolicy::Finite && total_inodes < limit.max_inodes())
            || (medium == StorageMedium::Tmpfs && fs_kind != StorageFilesystemKind::Tmpfs)
            || plan
                .roles()
                .iter()
                .filter(|role| role.pool_id() == pool_id)
                .any(|role| role.filesystem_instance_digest() != fs_digest)
        {
            return Err(HostError::Journal);
        }
    }
    if count != profile.pools().len() {
        return Err(HostError::Journal);
    }
    Ok(())
}

pub(in crate::journal::connection::reservation::storage_budget) async fn validate_orphan_roles(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM capacity_reservation_storage_roles AS r LEFT JOIN capacity_reservation_storage AS s ON s.reservation_id = r.reservation_id WHERE s.reservation_id IS NULL",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?
        == 0
    {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

pub(in crate::journal::connection::reservation::storage_budget) async fn validate_orphan_samples(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM capacity_reservation_storage_samples AS s LEFT JOIN capacity_reservation_storage AS r ON r.reservation_id = s.reservation_id WHERE r.reservation_id IS NULL",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?
        == 0
    {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}
