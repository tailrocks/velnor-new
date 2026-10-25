//! Provisioning admission capacity checks.

use super::{
    HostError, ReservationRequest, StorageInodePolicy, StorageMedium, StorageProfile,
    capacity_limits, capacity_usage,
};

pub(super) async fn has_legacy_active_reservation(
    connection: &turso::Connection,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM capacity_reservations AS r WHERE r.state != 'released' AND NOT EXISTS (SELECT 1 FROM capacity_reservation_provisioning AS p WHERE p.reservation_id = r.id) LIMIT 1",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_some())
}

pub(super) async fn capacity_fits(
    connection: &turso::Connection,
    request: ReservationRequest<'_>,
    retained_memory: u64,
) -> Result<bool, HostError> {
    let limits = capacity_limits(connection).await?;
    let usage = capacity_usage(connection).await?;
    let requested_memory = request
        .memory_bytes
        .checked_add(retained_memory)
        .ok_or(HostError::Journal)?;
    Ok(usage.jobs < limits.jobs
        && usage
            .cpu_millicores
            .checked_add(request.cpu_millicores)
            .is_some_and(|value| value <= limits.cpu_millicores)
        && usage
            .memory_bytes
            .checked_add(requested_memory)
            .is_some_and(|value| value <= limits.memory_bytes))
}

pub(super) fn profile_retained_memory(profile: &StorageProfile) -> Result<u64, HostError> {
    profile
        .roles()
        .iter()
        .filter(|role| role.alias_of().is_none())
        .try_fold(0_u64, |sum, role| {
            sum.checked_add(role.retained_memory_bytes())
                .ok_or(HostError::Journal)
        })
}

pub(super) async fn disk_pool_fits(
    connection: &turso::Connection,
    profile: &StorageProfile,
    sample: &super::super::super::storage_budget::StorageAdmissionSample,
) -> Result<bool, HostError> {
    let pool = profile
        .pools()
        .iter()
        .find(|pool| pool.medium() == StorageMedium::Disk)
        .ok_or(HostError::Journal)?;
    let observation = sample
        .pools()
        .iter()
        .find(|observation| observation.pool_id() == pool.pool_id())
        .ok_or(HostError::Journal)?;
    if observation.medium() != StorageMedium::Disk
        || observation.total_bytes() < pool.max_bytes()
        || observation.available_bytes() > observation.total_bytes()
        || (pool.inode_policy() == StorageInodePolicy::DynamicBtrfs
            && (observation.filesystem_kind()
                != super::super::super::storage_budget::StorageFilesystemKind::Btrfs
                || observation.total_inodes() != 0
                || observation.available_inodes() != 0))
    {
        return Ok(false);
    }
    let missing_details = scalar_count(
        connection,
        "SELECT COUNT(*) FROM capacity_reservations AS r WHERE r.state != 'released' AND NOT EXISTS (SELECT 1 FROM capacity_reservation_storage AS s WHERE s.reservation_id = r.id) AND NOT EXISTS (SELECT 1 FROM capacity_reservation_provisioning AS p WHERE p.reservation_id = r.id)",
    )
    .await?;
    if missing_details != 0 {
        return Ok(false);
    }
    let fs_mismatch = scalar_count_with_text(
        connection,
        "SELECT (SELECT COUNT(*) FROM capacity_reservation_storage_roles AS x JOIN capacity_reservations AS r ON r.id = x.reservation_id WHERE r.state != 'released' AND x.pool_id = ?1 AND x.filesystem_instance_digest != ?2) + (SELECT COUNT(*) FROM capacity_reservation_provisioning_pools AS x JOIN capacity_reservations AS r ON r.id = x.reservation_id WHERE r.state != 'released' AND x.pool_id = ?1 AND x.filesystem_instance_digest IS NOT ?2)",
        pool.pool_id(),
        observation.filesystem_instance_digest(),
    )
    .await?;
    if fs_mismatch != 0 {
        return Ok(false);
    }
    let held = sum_held_disk_charge(connection, pool.pool_id(), None).await?;
    let new_charge = profile
        .roles()
        .iter()
        .filter(|role| role.alias_of().is_none() && role.pool_id() == pool.pool_id())
        .try_fold((0_u64, 0_u64), |sum, role| {
            Ok::<_, HostError>((
                sum.0
                    .checked_add(role.max_bytes())
                    .ok_or(HostError::Journal)?,
                sum.1
                    .checked_add(role.max_inodes())
                    .ok_or(HostError::Journal)?,
            ))
        })?;
    let required_bytes = checked_required_charge(held.0, new_charge.0, pool.reserve_bytes())?;
    if !charge_fits_pool(
        required_bytes,
        pool.max_bytes(),
        observation.available_bytes(),
    ) {
        return Ok(false);
    }
    if pool.inode_policy() == StorageInodePolicy::Finite {
        let required_inodes = checked_required_charge(held.1, new_charge.1, pool.reserve_inodes())?;
        if observation.total_inodes() < pool.max_inodes()
            || !charge_fits_pool(
                required_inodes,
                pool.max_inodes(),
                observation.available_inodes(),
            )
        {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(in crate::journal::connection::reservation::provisioning) async fn metadata_capacity_fits(
    connection: &turso::Connection,
    reservation_id: i64,
    profile: &StorageProfile,
    sample: &super::super::super::storage_budget::StorageAdmissionSample,
) -> Result<bool, HostError> {
    let missing_details = scalar_count_with_optional_reservation(
        connection,
        "SELECT COUNT(*) FROM capacity_reservations AS r WHERE r.state != 'released' AND r.id != ?1 AND NOT EXISTS (SELECT 1 FROM capacity_reservation_storage AS s WHERE s.reservation_id = r.id) AND NOT EXISTS (SELECT 1 FROM capacity_reservation_provisioning AS p WHERE p.reservation_id = r.id)",
        reservation_id,
    )
    .await?;
    if missing_details != 0 {
        return Ok(false);
    }
    for pool in profile
        .pools()
        .iter()
        .filter(|pool| pool.medium() == StorageMedium::Disk)
    {
        if pool.inode_policy() == StorageInodePolicy::Unknown {
            return Ok(false);
        }
        let observation = sample
            .pools()
            .iter()
            .find(|observation| observation.pool_id() == pool.pool_id())
            .ok_or(HostError::Journal)?;
        if observation.total_bytes() < pool.max_bytes()
            || (pool.inode_policy() == StorageInodePolicy::Finite
                && observation.total_inodes() < pool.max_inodes())
        {
            return Ok(false);
        }
        let mismatch = scalar_count_with_pool_exclusion(
            connection,
            "SELECT (SELECT COUNT(*) FROM capacity_reservation_storage_roles AS x JOIN capacity_reservations AS r ON r.id = x.reservation_id WHERE r.state != 'released' AND r.id != ?1 AND x.pool_id = ?2 AND x.filesystem_instance_digest != ?3) + (SELECT COUNT(*) FROM capacity_reservation_provisioning_pools AS x JOIN capacity_reservations AS r ON r.id = x.reservation_id WHERE r.state != 'released' AND r.id != ?1 AND x.pool_id = ?2 AND x.filesystem_instance_digest IS NOT ?3)",
            reservation_id,
            pool.pool_id(),
            observation.filesystem_instance_digest(),
        )
        .await?;
        if mismatch != 0 {
            return Ok(false);
        }
        let held = sum_held_disk_charge(connection, pool.pool_id(), Some(reservation_id)).await?;
        let required_bytes = checked_required_charge(held.0, 0, pool.reserve_bytes())?;
        if !charge_fits_pool(
            required_bytes,
            pool.max_bytes(),
            observation.available_bytes(),
        ) {
            return Ok(false);
        }
        if pool.inode_policy() == StorageInodePolicy::Finite {
            let required_inodes = checked_required_charge(held.1, 0, pool.reserve_inodes())?;
            if !charge_fits_pool(
                required_inodes,
                pool.max_inodes(),
                observation.available_inodes(),
            ) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

pub(in crate::journal::connection::reservation::provisioning) fn checked_required_charge(
    held: u64,
    next: u64,
    reserve: u64,
) -> Result<u64, HostError> {
    held.checked_add(next)
        .and_then(|sum| sum.checked_add(reserve))
        .ok_or(HostError::Journal)
}

pub(in crate::journal::connection::reservation::provisioning) fn charge_fits_pool(
    required: u64,
    profile_limit: u64,
    observed_available: u64,
) -> bool {
    required <= profile_limit && required <= observed_available
}

async fn sum_held_disk_charge(
    connection: &turso::Connection,
    pool_id: &str,
    excluded_reservation_id: Option<i64>,
) -> Result<(u64, u64), HostError> {
    let mut rows = connection
        .query(
            "SELECT COALESCE(SUM(x.charge_bytes), 0), COALESCE(SUM(x.charge_inodes), 0) FROM capacity_reservation_storage_roles AS x JOIN capacity_reservations AS r ON r.id = x.reservation_id WHERE r.state != 'released' AND x.pool_id = ?1 AND (?2 IS NULL OR r.id != ?2)",
            (pool_id, excluded_reservation_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let mut total = (
        sql_u64(row.get(0).map_err(|_| HostError::Journal)?)?,
        sql_u64(row.get(1).map_err(|_| HostError::Journal)?)?,
    );
    let mut rows = connection
        .query(
            "SELECT COALESCE(SUM(x.charge_bytes), 0), COALESCE(SUM(x.charge_inodes), 0) FROM capacity_reservation_provisioning_roles AS x JOIN capacity_reservations AS r ON r.id = x.reservation_id WHERE r.state != 'released' AND x.pool_id = ?1 AND (?2 IS NULL OR r.id != ?2)",
            (pool_id, excluded_reservation_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    total.0 = total
        .0
        .checked_add(sql_u64(row.get(0).map_err(|_| HostError::Journal)?)?)
        .ok_or(HostError::Journal)?;
    total.1 = total
        .1
        .checked_add(sql_u64(row.get(1).map_err(|_| HostError::Journal)?)?)
        .ok_or(HostError::Journal)?;
    Ok(total)
}

async fn scalar_count(connection: &turso::Connection, sql: &str) -> Result<u64, HostError> {
    let mut rows = connection
        .query(sql, ())
        .await
        .map_err(|_| HostError::Journal)?;
    let value = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    u64::try_from(value).map_err(|_| HostError::Journal)
}

async fn scalar_count_with_text(
    connection: &turso::Connection,
    sql: &str,
    first: &str,
    second: &str,
) -> Result<u64, HostError> {
    let mut rows = connection
        .query(sql, (first, second))
        .await
        .map_err(|_| HostError::Journal)?;
    let value = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    u64::try_from(value).map_err(|_| HostError::Journal)
}

async fn scalar_count_with_optional_reservation(
    connection: &turso::Connection,
    sql: &str,
    reservation_id: i64,
) -> Result<u64, HostError> {
    let mut rows = connection
        .query(sql, [reservation_id])
        .await
        .map_err(|_| HostError::Journal)?;
    let value = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    u64::try_from(value).map_err(|_| HostError::Journal)
}

async fn scalar_count_with_pool_exclusion(
    connection: &turso::Connection,
    sql: &str,
    reservation_id: i64,
    pool_id: &str,
    filesystem_instance_digest: &str,
) -> Result<u64, HostError> {
    let mut rows = connection
        .query(sql, (reservation_id, pool_id, filesystem_instance_digest))
        .await
        .map_err(|_| HostError::Journal)?;
    let value = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    u64::try_from(value).map_err(|_| HostError::Journal)
}

pub(in crate::journal::connection::reservation::provisioning) fn sql_i64(
    value: u64,
) -> Result<i64, HostError> {
    i64::try_from(value).map_err(|_| HostError::Journal)
}

pub(in crate::journal::connection::reservation::provisioning) fn sql_u64(
    value: i64,
) -> Result<u64, HostError> {
    u64::try_from(value).map_err(|_| HostError::Journal)
}
