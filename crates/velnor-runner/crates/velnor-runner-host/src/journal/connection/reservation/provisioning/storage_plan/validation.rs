use std::collections::BTreeMap;

use crate::error::HostError;

use super::super::super::storage_budget::{
    StorageFilesystemKind, StorageInodePolicy, StorageMedium, StoragePoolLimit, StorageProfile,
};
use super::StorageProvisioningRole;

pub(super) struct PoolRow {
    scope: String,
    medium: StorageMedium,
    filesystem_kind: StorageFilesystemKind,
    inode_policy: StorageInodePolicy,
    limits: [u64; 4],
    digest: Option<String>,
    sample: [Option<u64>; 4],
}

pub(super) async fn validate_pool_rows(
    connection: &turso::Connection,
    reservation_id: i64,
    profile: &StorageProfile,
) -> Result<BTreeMap<String, PoolRow>, HostError> {
    let mut rows = connection
        .query(
            "SELECT pool_id, scope, medium, filesystem_kind, inode_policy, max_bytes, max_inodes, reserve_bytes, reserve_inodes, filesystem_instance_digest, total_bytes, available_bytes, total_inodes, available_inodes FROM capacity_reservation_provisioning_pools WHERE reservation_id = ?1 ORDER BY pool_id",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut stored = BTreeMap::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let pool_id: String = row.get(0).map_err(|_| HostError::Journal)?;
        let pool_row = PoolRow {
            scope: row.get(1).map_err(|_| HostError::Journal)?,
            medium: StorageMedium::parse(&row.get::<String>(2).map_err(|_| HostError::Journal)?)?,
            filesystem_kind: StorageFilesystemKind::parse(
                &row.get::<String>(3).map_err(|_| HostError::Journal)?,
            )?,
            inode_policy: StorageInodePolicy::parse(
                &row.get::<String>(4).map_err(|_| HostError::Journal)?,
            )?,
            limits: [
                positive_u64(row.get(5).map_err(|_| HostError::Journal)?)?,
                nonnegative_u64(row.get(6).map_err(|_| HostError::Journal)?)?,
                nonnegative_u64(row.get(7).map_err(|_| HostError::Journal)?)?,
                nonnegative_u64(row.get(8).map_err(|_| HostError::Journal)?)?,
            ],
            digest: row.get(9).map_err(|_| HostError::Journal)?,
            sample: [
                optional_nonnegative_u64(row.get(10).map_err(|_| HostError::Journal)?)?,
                optional_nonnegative_u64(row.get(11).map_err(|_| HostError::Journal)?)?,
                optional_nonnegative_u64(row.get(12).map_err(|_| HostError::Journal)?)?,
                optional_nonnegative_u64(row.get(13).map_err(|_| HostError::Journal)?)?,
            ],
        };
        if stored.insert(pool_id, pool_row).is_some() {
            return Err(HostError::Journal);
        }
    }
    if stored.len() != profile.pools().len() {
        return Err(HostError::Journal);
    }
    for pool in profile.pools() {
        let row = stored.get(pool.pool_id()).ok_or(HostError::Journal)?;
        validate_pool_row(row, pool)?;
    }
    Ok(stored)
}

pub(super) fn validate_role(
    profile: &StorageProfile,
    charge: &StorageProvisioningRole,
    pool_row: &PoolRow,
) -> Result<(), HostError> {
    let contract = charge.role().contract().ok_or(HostError::Journal)?;
    let limit = profile.role_limit(charge.role())?;
    let pool = profile.pool_limit(charge.pool_id())?;
    if limit.alias_of().is_some()
        || limit.role() != charge.role()
        || limit.pool_id() != charge.pool_id()
        || limit.medium() != charge.medium()
        || contract.medium != charge.medium()
        || pool.medium() != charge.medium()
        || pool.inode_policy() != charge.inode_policy()
        || limit.max_bytes() != charge.charge_bytes()
        || limit.max_inodes() != charge.charge_inodes()
        || limit.retained_memory_bytes() != charge.retained_memory_bytes()
        || pool_row.filesystem_kind != charge.filesystem_kind()
    {
        return Err(HostError::Journal);
    }
    let planned_digest = charge.planned_filesystem_instance_digest();
    match charge.medium() {
        StorageMedium::Disk => {
            if !matches!(
                charge.filesystem_kind(),
                StorageFilesystemKind::Btrfs | StorageFilesystemKind::Other
            ) || planned_digest
                .is_none_or(|value| !super::super::super::storage_budget::valid_digest(value))
                || planned_digest != pool_row.digest.as_deref()
                || (charge.inode_policy() == StorageInodePolicy::DynamicBtrfs
                    && charge.filesystem_kind() != StorageFilesystemKind::Btrfs)
                || charge.retained_memory_bytes() != 0
            {
                return Err(HostError::Journal);
            }
        }
        StorageMedium::Tmpfs => {
            if charge.filesystem_kind() != StorageFilesystemKind::Tmpfs
                || charge.inode_policy() != StorageInodePolicy::Finite
                || planned_digest.is_some()
                || charge.retained_memory_bytes() != charge.charge_bytes()
            {
                return Err(HostError::Journal);
            }
        }
    }
    Ok(())
}

fn validate_pool_row(row: &PoolRow, pool: &StoragePoolLimit) -> Result<(), HostError> {
    let expected_scope = match pool.medium() {
        StorageMedium::Disk => "shared-disk",
        StorageMedium::Tmpfs => "exclusive-tmpfs",
    };
    let expected_limits = [
        pool.max_bytes(),
        pool.max_inodes(),
        pool.reserve_bytes(),
        pool.reserve_inodes(),
    ];
    let valid_filesystem = match pool.medium() {
        StorageMedium::Disk => matches!(
            row.filesystem_kind,
            StorageFilesystemKind::Btrfs | StorageFilesystemKind::Other
        ),
        StorageMedium::Tmpfs => row.filesystem_kind == StorageFilesystemKind::Tmpfs,
    };
    let valid_sample = match (pool.medium(), pool.inode_policy()) {
        (StorageMedium::Disk, StorageInodePolicy::Finite) => {
            row.digest
                .as_deref()
                .is_some_and(super::super::super::storage_budget::valid_digest)
                && row.sample.iter().all(Option::is_some)
                && row.sample[0].is_some_and(|total| total > 0)
                && row.sample[1]
                    .zip(row.sample[0])
                    .is_some_and(|(available, total)| available <= total)
                && row.sample[2].is_some_and(|total| total > 0)
                && row.sample[3]
                    .zip(row.sample[2])
                    .is_some_and(|(available, total)| available <= total)
        }
        (StorageMedium::Disk, StorageInodePolicy::DynamicBtrfs) => {
            row.digest
                .as_deref()
                .is_some_and(super::super::super::storage_budget::valid_digest)
                && row.sample.iter().all(Option::is_some)
                && row.sample[2] == Some(0)
                && row.sample[3] == Some(0)
        }
        (StorageMedium::Disk, StorageInodePolicy::Unknown) => false,
        (StorageMedium::Tmpfs, StorageInodePolicy::Finite) => {
            row.digest.is_none() && row.sample.iter().all(Option::is_none)
        }
        (StorageMedium::Tmpfs, _) => false,
    };
    if row.scope != expected_scope
        || row.medium != pool.medium()
        || row.inode_policy != pool.inode_policy()
        || row.limits != expected_limits
        || !valid_filesystem
        || !valid_sample
        || (row.inode_policy == StorageInodePolicy::DynamicBtrfs
            && row.filesystem_kind != StorageFilesystemKind::Btrfs)
        || row.sample[0].is_some_and(|total| total == 0)
        || row.sample[1]
            .zip(row.sample[0])
            .is_some_and(|(available, total)| available > total)
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

pub(super) fn positive_u64(value: i64) -> Result<u64, HostError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(HostError::Journal)
}

pub(super) fn nonnegative_u64(value: i64) -> Result<u64, HostError> {
    u64::try_from(value).map_err(|_| HostError::Journal)
}

fn optional_nonnegative_u64(value: Option<i64>) -> Result<Option<u64>, HostError> {
    value.map(nonnegative_u64).transpose()
}
