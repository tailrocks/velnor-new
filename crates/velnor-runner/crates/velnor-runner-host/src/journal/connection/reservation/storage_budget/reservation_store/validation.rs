//! Stored reservation validation.

use crate::error::HostError;
use std::collections::BTreeMap;

use super::super::plan::{StorageReservationPlan, StorageRoleCharge};
use super::super::profile::StorageProfile;
use super::super::profile_store::{self, from_sql};
use super::super::role::{StorageFilesystemKind, StorageInodePolicy, StorageMedium, StorageRole};
use super::super::{valid_digest, valid_generation};
use super::{plan_memory, validate_orphan_roles, validate_orphan_samples, validate_sample_history};

/// Rebuild every configured profile and every recorded charge before opening the journal.
pub(in crate::journal::connection::reservation::storage_budget) async fn validate_rows(
    connection: &turso::Connection,
) -> Result<(), HostError> {
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

pub(in crate::journal::connection::reservation::storage_budget) async fn load_plan(
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

pub(in crate::journal::connection::reservation::storage_budget) async fn validate_role_shape(
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
