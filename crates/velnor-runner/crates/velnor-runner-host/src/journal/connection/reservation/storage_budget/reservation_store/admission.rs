//! Shared storage admission.

use std::time::Instant;

use crate::error::HostError;

use super::super::profile::StorageProfile;
use super::super::profile_store::{self, from_sql};
use super::super::role::{StorageFilesystemKind, StorageInodePolicy, StorageMedium};
use super::super::sample::StorageAdmission;
use super::{HeldCharges, HeldPool, load_plan, plan_memory};

/// Check a new sample against all outstanding reservations across connections.
pub(in crate::journal::connection::reservation::storage_budget) async fn admission_fits(
    connection: &turso::Connection,
    admission: &StorageAdmission,
    sample_generation_id: &str,
    engine_id: &str,
    docker_root_digest: &str,
    base_memory_charge: u64,
    excluded_reservation_id: Option<i64>,
) -> Result<bool, HostError> {
    let Some(profile) = profile_store::current(connection).await? else {
        return Ok(false);
    };
    if profile.generation_id() != admission.plan.profile_generation_id()
        || profile.profile_digest() != admission.plan.profile_digest()
    {
        return Ok(false);
    }
    let Some(held) = load_held_charges(connection, excluded_reservation_id).await? else {
        return Ok(false);
    };
    let extra_memory = base_memory_charge
        .checked_add(held.memory_bytes)
        .ok_or(HostError::Journal)?;
    admission.sample.validate_for(
        &profile,
        &admission.plan,
        sample_generation_id,
        engine_id,
        docker_root_digest,
        extra_memory,
        Instant::now(),
    )?;
    aggregate_fits(&profile, admission, &held)
}

pub(in crate::journal::connection::reservation::storage_budget) async fn matches_existing(
    connection: &turso::Connection,
    reservation_id: i64,
    admission: &StorageAdmission,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT profile_generation_id, profile_digest, instance_binding_digest, retained_memory_bytes FROM capacity_reservation_storage WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let generation: String = row.get(0).map_err(|_| HostError::Journal)?;
    let digest: String = row.get(1).map_err(|_| HostError::Journal)?;
    let instance_digest: String = row.get(2).map_err(|_| HostError::Journal)?;
    let retained: u64 = from_sql(row.get(3).map_err(|_| HostError::Journal)?)?;
    if generation != admission.plan.profile_generation_id()
        || digest != admission.plan.profile_digest()
        || instance_digest != admission.plan.instance_binding_digest()
        || retained != plan_memory(&admission.plan)?
    {
        return Ok(false);
    }
    let Some(current) = profile_store::current(connection).await? else {
        return Ok(false);
    };
    if current.generation_id() != generation || current.profile_digest() != digest {
        return Ok(false);
    }
    let stored = load_plan(connection, reservation_id, &current, &instance_digest).await?;
    Ok(stored == admission.plan)
}

async fn load_held_charges(
    connection: &turso::Connection,
    excluded_reservation_id: Option<i64>,
) -> Result<Option<HeldCharges>, HostError> {
    let mut rows = connection
        .query(
            "SELECT r.id, r.memory_bytes, s.reservation_id, s.retained_memory_bytes FROM capacity_reservations AS r LEFT JOIN capacity_reservation_storage AS s ON s.reservation_id = r.id WHERE r.state != 'released' ORDER BY r.id",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut held = HeldCharges::default();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let reservation_id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
        if Some(reservation_id) == excluded_reservation_id {
            continue;
        }
        let storage_reservation_id: Option<i64> = row.get(2).map_err(|_| HostError::Journal)?;
        if storage_reservation_id.is_none() {
            return Ok(None);
        }
        let base = from_sql(row.get(1).map_err(|_| HostError::Journal)?)?;
        let retained = from_sql(row.get(3).map_err(|_| HostError::Journal)?)?;
        held.memory_bytes = held
            .memory_bytes
            .checked_add(base)
            .and_then(|v| v.checked_add(retained))
            .ok_or(HostError::Journal)?;
    }
    let mut rows = connection
        .query(
            "SELECT r.id, d.pool_id, d.filesystem_instance_digest, d.medium, d.inode_policy, d.filesystem_kind, d.charge_bytes, d.charge_inodes FROM capacity_reservations AS r JOIN capacity_reservation_storage_roles AS d ON d.reservation_id = r.id WHERE r.state != 'released' ORDER BY r.id, d.role",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let reservation_id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
        if Some(reservation_id) == excluded_reservation_id {
            continue;
        }
        let pool_id: String = row.get(1).map_err(|_| HostError::Journal)?;
        let fs_digest: String = row.get(2).map_err(|_| HostError::Journal)?;
        let medium = StorageMedium::parse(&row.get::<String>(3).map_err(|_| HostError::Journal)?)?;
        let policy =
            StorageInodePolicy::parse(&row.get::<String>(4).map_err(|_| HostError::Journal)?)?;
        let filesystem_kind =
            StorageFilesystemKind::parse(&row.get::<String>(5).map_err(|_| HostError::Journal)?)?;
        let bytes = from_sql(row.get(6).map_err(|_| HostError::Journal)?)?;
        let inodes = from_sql(row.get(7).map_err(|_| HostError::Journal)?)?;
        let charge = held.by_pool.entry(pool_id).or_insert_with(|| HeldPool {
            filesystem_instance_digest: fs_digest.clone(),
            medium,
            inode_policy: policy,
            filesystem_kind,
            bytes: 0,
            inodes: 0,
        });
        if charge.filesystem_instance_digest != fs_digest
            || charge.medium != medium
            || charge.inode_policy != policy
            || charge.filesystem_kind != filesystem_kind
        {
            return Ok(None);
        }
        charge.bytes = charge.bytes.checked_add(bytes).ok_or(HostError::Journal)?;
        charge.inodes = charge
            .inodes
            .checked_add(inodes)
            .ok_or(HostError::Journal)?;
    }
    Ok(Some(held))
}

fn aggregate_fits(
    profile: &StorageProfile,
    admission: &StorageAdmission,
    held: &HeldCharges,
) -> Result<bool, HostError> {
    if !held_charges_match_profile(profile, admission, held) {
        return Ok(false);
    }
    for limit in profile.pools() {
        if !pool_aggregate_fits(admission, held, limit)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn held_charges_match_profile(
    profile: &StorageProfile,
    admission: &StorageAdmission,
    held: &HeldCharges,
) -> bool {
    for (pool_id, charge) in &held.by_pool {
        let Some(limit) = profile
            .pools()
            .iter()
            .find(|pool| pool.pool_id() == pool_id)
        else {
            return false;
        };
        let Some(observation) = admission
            .sample
            .pools()
            .iter()
            .find(|pool| pool.pool_id() == pool_id)
        else {
            return false;
        };
        if observation.filesystem_instance_digest() != charge.filesystem_instance_digest
            || observation.medium() != charge.medium
            || observation.filesystem_kind() != charge.filesystem_kind
            || limit.inode_policy() != charge.inode_policy
        {
            return false;
        }
    }
    true
}

fn pool_aggregate_fits(
    admission: &StorageAdmission,
    held: &HeldCharges,
    limit: &super::super::profile::StoragePoolLimit,
) -> Result<bool, HostError> {
    let planned = planned_charge(admission, limit.pool_id())?;
    let held = held.by_pool.get(limit.pool_id());
    let bytes = held
        .map_or(0, |charge| charge.bytes)
        .checked_add(planned.0)
        .ok_or(HostError::Journal)?;
    let inodes = held
        .map_or(0, |charge| charge.inodes)
        .checked_add(planned.1)
        .ok_or(HostError::Journal)?;
    if !fits_profile_pool(limit, bytes, inodes) {
        return Ok(false);
    }
    let observation = admission
        .sample
        .pools()
        .iter()
        .find(|pool| pool.pool_id() == limit.pool_id())
        .ok_or(HostError::Journal)?;
    if bytes
        .checked_add(limit.reserve_bytes())
        .is_none_or(|sum| sum > observation.available_bytes())
    {
        return Ok(false);
    }
    if limit.inode_policy() == StorageInodePolicy::Finite
        && inodes
            .checked_add(limit.reserve_inodes())
            .is_none_or(|sum| sum > observation.available_inodes())
    {
        return Ok(false);
    }
    Ok(true)
}

fn planned_charge(admission: &StorageAdmission, pool_id: &str) -> Result<(u64, u64), HostError> {
    admission
        .plan
        .roles()
        .iter()
        .filter(|role| role.pool_id() == pool_id)
        .try_fold((0_u64, 0_u64), |sum, role| {
            Ok::<_, HostError>((
                sum.0
                    .checked_add(role.charge_bytes())
                    .ok_or(HostError::Journal)?,
                sum.1
                    .checked_add(role.charge_inodes())
                    .ok_or(HostError::Journal)?,
            ))
        })
}

fn fits_profile_pool(
    limit: &super::super::profile::StoragePoolLimit,
    bytes: u64,
    inodes: u64,
) -> bool {
    bytes
        .checked_add(limit.reserve_bytes())
        .is_some_and(|sum| sum <= limit.max_bytes())
        && (limit.inode_policy() != StorageInodePolicy::Finite
            || inodes
                .checked_add(limit.reserve_inodes())
                .is_some_and(|sum| sum <= limit.max_inodes()))
}
