//! Durable storage plans and their fresh admission observations.

use std::collections::BTreeMap;
use std::time::Instant;

use crate::error::HostError;

use super::observations::StoragePoolObservation;
use super::plan::{StorageReservationPlan, StorageRoleCharge};
use super::profile::StorageProfile;
use super::profile_store::{self, from_sql, to_sql};
use super::role::{StorageFilesystemKind, StorageInodePolicy, StorageMedium, StorageRole};
use super::sample::{StorageAdmission, StorageAdmissionSample};
use super::{valid_digest, valid_generation};

#[derive(Default)]
struct HeldCharges {
    by_pool: BTreeMap<String, HeldPool>,
    memory_bytes: u64,
}

#[derive(Clone)]
struct HeldPool {
    filesystem_instance_digest: String,
    medium: StorageMedium,
    inode_policy: StorageInodePolicy,
    filesystem_kind: StorageFilesystemKind,
    bytes: u64,
    inodes: u64,
}

/// Rebuild every configured profile and every recorded charge before opening the journal.
pub(super) async fn validate_rows(connection: &turso::Connection) -> Result<(), HostError> {
    validate_profiles(connection).await?;
    validate_reservations(connection).await
}

async fn validate_profiles(connection: &turso::Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT generation_id FROM host_storage_profiles ORDER BY generation_id",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let generation: String = row.get(0).map_err(|_| HostError::Journal)?;
        profile_store::load(connection, &generation)
            .await?
            .ok_or(HostError::Journal)?;
    }
    if let Some(generation) = profile_store::current_generation(connection).await? {
        profile_store::load(connection, &generation)
            .await?
            .ok_or(HostError::Journal)?;
    }
    Ok(())
}

async fn validate_reservations(connection: &turso::Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT reservation_id, profile_generation_id, profile_digest, instance_binding_digest, retained_memory_bytes FROM capacity_reservation_storage ORDER BY reservation_id",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let reservation_id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
        let generation: String = row.get(1).map_err(|_| HostError::Journal)?;
        let digest: String = row.get(2).map_err(|_| HostError::Journal)?;
        let instance_digest: String = row.get(3).map_err(|_| HostError::Journal)?;
        let retained_memory = from_sql(row.get(4).map_err(|_| HostError::Journal)?)?;
        if !valid_generation(&generation)
            || !valid_digest(&digest)
            || !valid_digest(&instance_digest)
        {
            return Err(HostError::Journal);
        }
        let profile = profile_store::load(connection, &generation)
            .await?
            .ok_or(HostError::Journal)?;
        if profile.profile_digest() != digest {
            return Err(HostError::Journal);
        }
        let plan = load_plan(connection, reservation_id, &profile, &instance_digest).await?;
        profile.validate_plan(&plan)?;
        if plan_memory(&plan)? != retained_memory {
            return Err(HostError::Journal);
        }
        let (engine_id, root_digest) =
            validate_parent_reservation(connection, reservation_id).await?;
        let role_kinds = validate_role_shape(connection, reservation_id, &profile).await?;
        validate_sample_history(
            connection,
            reservation_id,
            &profile,
            &plan,
            &engine_id,
            &root_digest,
            &role_kinds,
        )
        .await?;
    }
    validate_orphan_roles(connection).await?;
    validate_orphan_samples(connection).await?;
    Ok(())
}

async fn validate_parent_reservation(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<(String, String), HostError> {
    let mut rows = connection
        .query(
            "SELECT engine_id, docker_root_digest FROM capacity_reservations WHERE id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows.next().await.map_err(|_| HostError::Journal)?;
    let Some(row) = row else {
        return Err(HostError::Journal);
    };
    let engine_id: String = row.get(0).map_err(|_| HostError::Journal)?;
    let root_digest: String = row.get(1).map_err(|_| HostError::Journal)?;
    if engine_id.is_empty() || !valid_digest(&root_digest) {
        return Err(HostError::Journal);
    }
    Ok((engine_id, root_digest))
}

pub(super) async fn load_plan(
    connection: &turso::Connection,
    reservation_id: i64,
    profile: &StorageProfile,
    instance_digest: &str,
) -> Result<StorageReservationPlan, HostError> {
    let mut rows = connection
        .query(
        "SELECT role, pool_id, filesystem_instance_digest, medium, charge_bytes, charge_inodes, retained_memory_bytes FROM capacity_reservation_storage_roles WHERE reservation_id = ?1 ORDER BY role",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut roles = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let role = StorageRole::parse(&row.get::<String>(0).map_err(|_| HostError::Journal)?)?;
        let pool_id: String = row.get(1).map_err(|_| HostError::Journal)?;
        let filesystem_digest: String = row.get(2).map_err(|_| HostError::Journal)?;
        let medium = StorageMedium::parse(&row.get::<String>(3).map_err(|_| HostError::Journal)?)?;
        let charge_bytes = from_sql(row.get(4).map_err(|_| HostError::Journal)?)?;
        let charge_inodes = from_sql(row.get(5).map_err(|_| HostError::Journal)?)?;
        let retained_memory = from_sql(row.get(6).map_err(|_| HostError::Journal)?)?;
        roles.push(StorageRoleCharge::new(
            role,
            pool_id,
            filesystem_digest,
            medium,
            charge_bytes,
            charge_inodes,
            retained_memory,
        )?);
    }
    StorageReservationPlan::new(
        profile.generation_id().to_owned(),
        profile.profile_digest().to_owned(),
        instance_digest.to_owned(),
        roles,
    )
}

pub(super) async fn validate_role_shape(
    connection: &turso::Connection,
    reservation_id: i64,
    profile: &StorageProfile,
) -> Result<BTreeMap<String, StorageFilesystemKind>, HostError> {
    let mut rows = connection
        .query(
            "SELECT role, pool_id, medium, inode_policy, filesystem_kind FROM capacity_reservation_storage_roles WHERE reservation_id = ?1 ORDER BY role",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut kinds = BTreeMap::new();
    let mut count = 0;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        count += 1;
        let role = StorageRole::parse(&row.get::<String>(0).map_err(|_| HostError::Journal)?)?;
        let pool_id: String = row.get(1).map_err(|_| HostError::Journal)?;
        let medium = StorageMedium::parse(&row.get::<String>(2).map_err(|_| HostError::Journal)?)?;
        let inode_policy =
            StorageInodePolicy::parse(&row.get::<String>(3).map_err(|_| HostError::Journal)?)?;
        let filesystem_kind =
            StorageFilesystemKind::parse(&row.get::<String>(4).map_err(|_| HostError::Journal)?)?;
        let role_limit = profile
            .roles()
            .iter()
            .find(|limit| limit.role() == role && limit.alias_of().is_none())
            .ok_or(HostError::Journal)?;
        let pool_limit = profile
            .pools()
            .iter()
            .find(|limit| limit.pool_id() == pool_id)
            .ok_or(HostError::Journal)?;
        if role_limit.pool_id() != pool_id
            || role_limit.medium() != medium
            || pool_limit.inode_policy() != inode_policy
            || (medium == StorageMedium::Tmpfs && filesystem_kind != StorageFilesystemKind::Tmpfs)
            || (inode_policy == StorageInodePolicy::DynamicBtrfs
                && filesystem_kind != StorageFilesystemKind::Btrfs)
        {
            return Err(HostError::Journal);
        }
        if kinds
            .insert(pool_id, filesystem_kind)
            .is_some_and(|previous| previous != filesystem_kind)
        {
            return Err(HostError::Journal);
        }
    }
    let expected_count = profile
        .roles()
        .iter()
        .filter(|role| role.alias_of().is_none())
        .count();
    if count != expected_count {
        return Err(HostError::Journal);
    }
    Ok(kinds)
}

pub(super) async fn validate_sample_history(
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

async fn validate_orphan_roles(connection: &turso::Connection) -> Result<(), HostError> {
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

async fn validate_orphan_samples(connection: &turso::Connection) -> Result<(), HostError> {
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

/// Check a new sample against all outstanding reservations across connections.
pub(super) async fn admission_fits(
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

pub(super) async fn matches_existing(
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
    limit: &super::profile::StoragePoolLimit,
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
    if limit.inode_policy() == StorageInodePolicy::Finite {
        if inodes
            .checked_add(limit.reserve_inodes())
            .is_none_or(|sum| sum > observation.available_inodes())
        {
            return Ok(false);
        }
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

fn fits_profile_pool(limit: &super::profile::StoragePoolLimit, bytes: u64, inodes: u64) -> bool {
    bytes
        .checked_add(limit.reserve_bytes())
        .is_some_and(|sum| sum <= limit.max_bytes())
        && (limit.inode_policy() != StorageInodePolicy::Finite
            || inodes
                .checked_add(limit.reserve_inodes())
                .is_some_and(|sum| sum <= limit.max_inodes()))
}

/// Persist the immutable role charge and first successful admission sample.
pub(super) async fn insert_reservation(
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
pub(super) async fn append_sample(
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

pub(super) fn plan_memory(plan: &StorageReservationPlan) -> Result<u64, HostError> {
    plan.roles().iter().try_fold(0_u64, |sum, role| {
        sum.checked_add(role.retained_memory_bytes())
            .ok_or(HostError::Journal)
    })
}
