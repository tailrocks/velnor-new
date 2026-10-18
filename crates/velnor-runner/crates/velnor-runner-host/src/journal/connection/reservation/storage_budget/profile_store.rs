//! Durable immutable profile generations.

use crate::error::HostError;
use crate::journal::Journal;
use crate::journal::connection::storage::in_transaction;

use super::profile::{StoragePoolLimit, StorageProfile, StorageRoleLimit};
use super::role::{StorageInodePolicy, StorageMedium, StorageRole};

impl Journal {
    /// Record a new immutable storage policy generation.
    pub(crate) async fn configure_storage_profile(
        &self,
        profile: StorageProfile,
    ) -> Result<(), HostError> {
        in_transaction(self, async |connection| {
            configure(connection, &profile).await
        })
        .await
    }
}

pub(super) async fn configure(
    connection: &turso::Connection,
    profile: &StorageProfile,
) -> Result<(), HostError> {
    if let Some(stored) = load(connection, profile.generation_id()).await? {
        if stored != *profile
            || current_generation(connection).await?.as_deref() != Some(profile.generation_id())
        {
            return Err(HostError::Journal);
        }
        return Ok(());
    }
    insert_profile(connection, profile).await?;
    set_current_generation(connection, profile.generation_id()).await
}

pub(super) async fn current(
    connection: &turso::Connection,
) -> Result<Option<StorageProfile>, HostError> {
    let Some(generation) = current_generation(connection).await? else {
        return Ok(None);
    };
    load(connection, &generation).await
}

pub(super) async fn load(
    connection: &turso::Connection,
    generation: &str,
) -> Result<Option<StorageProfile>, HostError> {
    let Some(digest) = profile_digest(connection, generation).await? else {
        return Ok(None);
    };
    let pools = load_pools(connection, generation).await?;
    let roles = load_roles(connection, generation).await?;
    let profile = StorageProfile::new(generation.to_owned(), pools, roles)?;
    if profile.profile_digest() != digest {
        return Err(HostError::Journal);
    }
    Ok(Some(profile))
}

async fn insert_profile(
    connection: &turso::Connection,
    profile: &StorageProfile,
) -> Result<(), HostError> {
    connection
        .execute(
            "INSERT INTO host_storage_profiles (generation_id, profile_digest) VALUES (?1, ?2)",
            (
                profile.generation_id.as_str(),
                profile.profile_digest.as_str(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    for pool in &profile.pools {
        connection
            .execute(
                "INSERT INTO host_storage_profile_pools (generation_id, pool_id, medium, inode_policy, max_bytes, max_inodes, reserve_bytes, reserve_inodes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                (
                    profile.generation_id.as_str(),
                    pool.pool_id.as_str(),
                    pool.medium.as_str(),
                    pool.inode_policy.as_str(),
                    to_sql(pool.max_bytes)?,
                    to_sql(pool.max_inodes)?,
                    to_sql(pool.reserve_bytes)?,
                    to_sql(pool.reserve_inodes)?,
                ),
            )
            .await
            .map_err(|_| HostError::Journal)?;
    }
    for role in profile
        .roles
        .iter()
        .filter(|role| role.alias_of().is_none())
        .chain(
            profile
                .roles
                .iter()
                .filter(|role| role.alias_of().is_some()),
        )
    {
        connection
            .execute(
                "INSERT INTO host_storage_profile_roles (generation_id, role, pool_id, medium, max_bytes, max_inodes, retained_memory_bytes, alias_of) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                (
                    profile.generation_id.as_str(),
                    role.role.as_str(),
                    role.pool_id.as_str(),
                    role.medium.as_str(),
                    to_sql(role.max_bytes)?,
                    to_sql(role.max_inodes)?,
                    to_sql(role.retained_memory_bytes)?,
                    role.alias_of().map(StorageRole::as_str),
                ),
            )
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

async fn load_pools(
    connection: &turso::Connection,
    generation: &str,
) -> Result<Vec<StoragePoolLimit>, HostError> {
    let mut rows = connection
        .query(
            "SELECT pool_id, medium, inode_policy, max_bytes, max_inodes, reserve_bytes, reserve_inodes FROM host_storage_profile_pools WHERE generation_id = ?1 ORDER BY pool_id",
            [generation],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut pools = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        pools.push(StoragePoolLimit::new_with_inode_policy(
            row.get(0).map_err(|_| HostError::Journal)?,
            StorageMedium::parse(&row.get::<String>(1).map_err(|_| HostError::Journal)?)?,
            StorageInodePolicy::parse(&row.get::<String>(2).map_err(|_| HostError::Journal)?)?,
            from_sql(row.get(3).map_err(|_| HostError::Journal)?)?,
            from_sql(row.get(4).map_err(|_| HostError::Journal)?)?,
            from_sql(row.get(5).map_err(|_| HostError::Journal)?)?,
            from_sql(row.get(6).map_err(|_| HostError::Journal)?)?,
        )?);
    }
    Ok(pools)
}

async fn load_roles(
    connection: &turso::Connection,
    generation: &str,
) -> Result<Vec<StorageRoleLimit>, HostError> {
    let mut rows = connection
        .query(
            "SELECT role, pool_id, medium, max_bytes, max_inodes, retained_memory_bytes, alias_of FROM host_storage_profile_roles WHERE generation_id = ?1 ORDER BY role",
            [generation],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut roles = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let role = StorageRole::parse(&row.get::<String>(0).map_err(|_| HostError::Journal)?)?;
        let pool_id: String = row.get(1).map_err(|_| HostError::Journal)?;
        let medium = StorageMedium::parse(&row.get::<String>(2).map_err(|_| HostError::Journal)?)?;
        let max_bytes = from_sql(row.get(3).map_err(|_| HostError::Journal)?)?;
        let max_inodes = from_sql(row.get(4).map_err(|_| HostError::Journal)?)?;
        let retained_memory = from_sql(row.get(5).map_err(|_| HostError::Journal)?)?;
        let alias_of: Option<String> = row.get(6).map_err(|_| HostError::Journal)?;
        if let Some(alias_of) = alias_of {
            if retained_memory != 0 {
                return Err(HostError::Journal);
            }
            roles.push(StorageRoleLimit::new_alias(
                role,
                pool_id,
                medium,
                StorageRole::parse(&alias_of)?,
                max_bytes,
                max_inodes,
            )?);
        } else {
            roles.push(StorageRoleLimit::new(
                role,
                pool_id,
                medium,
                max_bytes,
                max_inodes,
                retained_memory,
            )?);
        }
    }
    Ok(roles)
}

async fn profile_digest(
    connection: &turso::Connection,
    generation: &str,
) -> Result<Option<String>, HostError> {
    let mut rows = connection
        .query(
            "SELECT profile_digest FROM host_storage_profiles WHERE generation_id = ?1",
            [generation],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .map(|row| row.get(0).map_err(|_| HostError::Journal))
        .transpose()
}

pub(super) async fn current_generation(
    connection: &turso::Connection,
) -> Result<Option<String>, HostError> {
    let mut rows = connection
        .query(
            "SELECT generation_id FROM host_storage_profile_current WHERE singleton = 1",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .map(|row| row.get(0).map_err(|_| HostError::Journal))
        .transpose()
}

async fn set_current_generation(
    connection: &turso::Connection,
    generation: &str,
) -> Result<(), HostError> {
    if current_generation(connection).await?.is_some() {
        connection
            .execute(
                "UPDATE host_storage_profile_current SET generation_id = ?1 WHERE singleton = 1",
                [generation],
            )
            .await
            .map_err(|_| HostError::Journal)?;
    } else {
        connection
            .execute(
                "INSERT INTO host_storage_profile_current (singleton, generation_id) VALUES (1, ?1)",
                [generation],
            )
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

pub(super) fn to_sql(value: u64) -> Result<i64, HostError> {
    i64::try_from(value).map_err(|_| HostError::Journal)
}

pub(super) fn from_sql(value: i64) -> Result<u64, HostError> {
    u64::try_from(value).map_err(|_| HostError::Journal)
}
