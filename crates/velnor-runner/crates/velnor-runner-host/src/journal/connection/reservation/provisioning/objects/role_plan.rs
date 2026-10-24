use crate::error::HostError;
use crate::journal::connection::{
    StorageFilesystemKind, StorageInodePolicy, StorageMedium, StoragePoolLimit, StorageProfile,
    StorageRole, StorageRoleLimit,
};

use super::super::HoldContext;

pub(super) struct ObjectRolePlan {
    #[cfg(test)]
    pub(super) pool_id: String,
    pub(super) medium: StorageMedium,
    pub(super) filesystem_kind: StorageFilesystemKind,
    pub(super) inode_policy: StorageInodePolicy,
    pub(super) charge_bytes: u64,
    pub(super) charge_inodes: u64,
    pub(super) retained_memory_bytes: u64,
    #[cfg(test)]
    pub(super) publication_policy_digest: Option<String>,
}

pub(super) async fn load_object_role_plan(
    connection: &turso::Connection,
    reservation_id: i64,
    role: StorageRole,
    context: &HoldContext,
) -> Result<ObjectRolePlan, HostError> {
    let profile = super::super::super::storage_budget::profile_generation(
        connection,
        &context.profile_generation_id,
    )
    .await?
    .ok_or(HostError::Journal)?;
    let current = super::super::super::storage_budget::current_profile(connection)
        .await?
        .ok_or(HostError::Journal)?;
    if profile.generation_id() != context.profile_generation_id
        || profile.profile_digest() != context.profile_digest
        || current != profile
    {
        return Err(HostError::Journal);
    }
    let role_limit = profile.role_limit(role)?;
    if role_limit.alias_of().is_some() {
        return Err(HostError::Journal);
    }
    let pool = profile.pool_limit(role_limit.pool_id())?;
    let persisted = read_persisted_role_plan(connection, reservation_id, role).await?;
    validate_persisted_role_plan(
        connection,
        reservation_id,
        role,
        &profile,
        role_limit,
        pool,
        persisted,
    )
    .await
}

struct PersistedObjectRolePlan {
    pool_id: String,
    medium: StorageMedium,
    filesystem_kind: StorageFilesystemKind,
    inode_policy: StorageInodePolicy,
    planned_filesystem_instance_digest: Option<String>,
    charge_bytes: u64,
    charge_inodes: u64,
    retained_memory_bytes: u64,
}

async fn read_persisted_role_plan(
    connection: &turso::Connection,
    reservation_id: i64,
    role: StorageRole,
) -> Result<PersistedObjectRolePlan, HostError> {
    let mut rows = connection
        .query(
            "SELECT pool_id, medium, filesystem_kind, inode_policy, planned_filesystem_instance_digest, charge_bytes, charge_inodes, retained_memory_bytes FROM capacity_reservation_provisioning_roles WHERE reservation_id = ?1 AND role = ?2",
            (reservation_id, role.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(PersistedObjectRolePlan {
        pool_id: row.get(0).map_err(|_| HostError::Journal)?,
        medium: StorageMedium::parse(&row.get::<String>(1).map_err(|_| HostError::Journal)?)?,
        filesystem_kind: StorageFilesystemKind::parse(
            &row.get::<String>(2).map_err(|_| HostError::Journal)?,
        )?,
        inode_policy: StorageInodePolicy::parse(
            &row.get::<String>(3).map_err(|_| HostError::Journal)?,
        )?,
        planned_filesystem_instance_digest: row.get(4).map_err(|_| HostError::Journal)?,
        charge_bytes: positive_sql_u64(row.get(5).map_err(|_| HostError::Journal)?)?,
        charge_inodes: positive_sql_u64(row.get(6).map_err(|_| HostError::Journal)?)?,
        retained_memory_bytes: nonnegative_sql_u64(row.get(7).map_err(|_| HostError::Journal)?)?,
    })
}

async fn validate_persisted_role_plan(
    connection: &turso::Connection,
    reservation_id: i64,
    role: StorageRole,
    profile: &StorageProfile,
    role_limit: &StorageRoleLimit,
    pool: &StoragePoolLimit,
    persisted: PersistedObjectRolePlan,
) -> Result<ObjectRolePlan, HostError> {
    let contract = role.contract().ok_or(HostError::Journal)?;
    let publication_policy_digest = if role == StorageRole::EngineMetadata {
        Some(
            profile
                .controller_publication()
                .ok_or(HostError::Journal)?
                .digest(),
        )
    } else {
        None
    };
    let medium_matches_contract = persisted.medium == contract.medium
        && match role {
            StorageRole::EngineMetadata => persisted.medium == StorageMedium::Disk,
            StorageRole::SharedRun
            | StorageRole::RunnerHome
            | StorageRole::RunnerWork
            | StorageRole::DindData => persisted.medium == StorageMedium::Tmpfs,
            _ => false,
        };
    let instance_matches = match persisted.medium {
        StorageMedium::Tmpfs => persisted.planned_filesystem_instance_digest.is_none(),
        StorageMedium::Disk => {
            persisted.planned_filesystem_instance_digest.as_deref()
                == Some(
                    pool_instance_digest(connection, reservation_id, role)
                        .await?
                        .as_str(),
                )
        }
    };
    let object_policy_matches =
        object_policy_digest(connection, reservation_id, role).await? == publication_policy_digest;
    if !medium_matches_contract
        || persisted.pool_id != role_limit.pool_id()
        || pool.medium() != persisted.medium
        || !instance_matches
        || (persisted.medium == StorageMedium::Tmpfs
            && persisted.filesystem_kind != StorageFilesystemKind::Tmpfs)
        || (persisted.medium == StorageMedium::Disk
            && !matches!(
                persisted.filesystem_kind,
                StorageFilesystemKind::Btrfs | StorageFilesystemKind::Other
            ))
        || (persisted.medium == StorageMedium::Tmpfs
            && pool.inode_policy() != StorageInodePolicy::Finite)
        || persisted.inode_policy != pool.inode_policy()
        || !object_policy_matches
        || persisted.charge_bytes != role_limit.max_bytes()
        || persisted.charge_inodes != role_limit.max_inodes()
        || persisted.retained_memory_bytes != role_limit.retained_memory_bytes()
    {
        return Err(HostError::Journal);
    }
    Ok(ObjectRolePlan {
        #[cfg(test)]
        pool_id: persisted.pool_id,
        medium: persisted.medium,
        filesystem_kind: persisted.filesystem_kind,
        inode_policy: persisted.inode_policy,
        charge_bytes: persisted.charge_bytes,
        charge_inodes: persisted.charge_inodes,
        retained_memory_bytes: persisted.retained_memory_bytes,
        #[cfg(test)]
        publication_policy_digest,
    })
}

async fn object_policy_digest(
    connection: &turso::Connection,
    reservation_id: i64,
    role: StorageRole,
) -> Result<Option<String>, HostError> {
    let mut rows = connection
        .query(
            "SELECT publication_policy_digest FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1 AND role = ?2",
            (reservation_id, role.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    row.get(0).map_err(|_| HostError::Journal)
}

async fn pool_instance_digest(
    connection: &turso::Connection,
    reservation_id: i64,
    role: StorageRole,
) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT pool.filesystem_instance_digest FROM capacity_reservation_provisioning_roles AS role JOIN capacity_reservation_provisioning_pools AS pool ON pool.reservation_id = role.reservation_id AND pool.pool_id = role.pool_id WHERE role.reservation_id = ?1 AND role.role = ?2 AND pool.scope = 'shared-disk'",
            (reservation_id, role.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    row.get::<Option<String>>(0)
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)
}

fn positive_sql_u64(value: i64) -> Result<u64, HostError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(HostError::Journal)
}

fn nonnegative_sql_u64(value: i64) -> Result<u64, HostError> {
    u64::try_from(value).map_err(|_| HostError::Journal)
}
