//! Storage role charges from one journal capacity reservation.

use std::collections::BTreeMap;

use crate::error::HostError;

use super::super::storage_budget::{
    StorageFilesystemKind, StorageInodePolicy, StorageMedium, StorageRole,
};
use super::HoldContext;
use validation::{nonnegative_u64, positive_u64, validate_pool_rows, validate_role};

mod validation;

const EXPECTED_ROLE_COUNT: usize = StorageRole::PHYSICAL.len();

/// One filesystem charge that the accepted storage profile specifies.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct StorageProvisioningRole {
    role: StorageRole,
    pool_id: String,
    medium: StorageMedium,
    filesystem_kind: StorageFilesystemKind,
    inode_policy: StorageInodePolicy,
    planned_filesystem_instance_digest: Option<String>,
    charge_bytes: u64,
    charge_inodes: u64,
    retained_memory_bytes: u64,
}

impl StorageProvisioningRole {
    pub(crate) const fn role(&self) -> StorageRole {
        self.role
    }

    pub(crate) fn pool_id(&self) -> &str {
        &self.pool_id
    }

    pub(crate) const fn medium(&self) -> StorageMedium {
        self.medium
    }

    pub(crate) const fn filesystem_kind(&self) -> StorageFilesystemKind {
        self.filesystem_kind
    }

    pub(crate) const fn inode_policy(&self) -> StorageInodePolicy {
        self.inode_policy
    }

    pub(crate) fn planned_filesystem_instance_digest(&self) -> Option<&str> {
        self.planned_filesystem_instance_digest.as_deref()
    }

    pub(crate) const fn charge_bytes(&self) -> u64 {
        self.charge_bytes
    }

    pub(crate) const fn charge_inodes(&self) -> u64 {
        self.charge_inodes
    }

    pub(crate) const fn retained_memory_bytes(&self) -> u64 {
        self.retained_memory_bytes
    }
}

/// Storage capacity reservation with its reservation ID, operation ID, and read-only storage profile.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct StorageProvisioningPlan {
    reservation_id: i64,
    operation_id: i64,
    scale_set_id: i64,
    engine_id: String,
    docker_root_digest: String,
    profile_generation_id: String,
    profile_digest: String,
    sample_generation_id: String,
    roles: Vec<StorageProvisioningRole>,
}

impl StorageProvisioningPlan {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn profile_generation_id(&self) -> &str {
        &self.profile_generation_id
    }

    pub(crate) fn profile_digest(&self) -> &str {
        &self.profile_digest
    }

    pub(crate) fn sample_generation_id(&self) -> &str {
        &self.sample_generation_id
    }

    pub(crate) fn roles(&self) -> &[StorageProvisioningRole] {
        &self.roles
    }
}

pub(super) async fn load(
    connection: &turso::Connection,
    reservation_id: i64,
    context: &HoldContext,
) -> Result<StorageProvisioningPlan, HostError> {
    if reservation_id <= 0 || context.operation_id <= 0 || context.scale_set_id <= 0 {
        return Err(HostError::Journal);
    }
    let profile = super::super::storage_budget::profile_generation(
        connection,
        &context.profile_generation_id,
    )
    .await?
    .ok_or(HostError::Journal)?;
    let current = super::super::storage_budget::current_profile(connection)
        .await?
        .ok_or(HostError::Journal)?;
    if current != profile
        || profile.generation_id() != context.profile_generation_id
        || profile.profile_digest() != context.profile_digest
    {
        return Err(HostError::Journal);
    }
    profile.validate_provisioning_contract()?;
    validate_reservation_identity(connection, reservation_id, context).await?;
    let pools = validate_pool_rows(connection, reservation_id, &profile).await?;

    let mut rows = connection
        .query(
            "SELECT role, pool_id, medium, filesystem_kind, inode_policy, planned_filesystem_instance_digest, charge_bytes, charge_inodes, retained_memory_bytes FROM capacity_reservation_provisioning_roles WHERE reservation_id = ?1 ORDER BY role",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut roles = Vec::with_capacity(EXPECTED_ROLE_COUNT);
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        roles.push(decode_and_validate_role(&row, &profile, &pools)?);
    }
    roles.sort_unstable_by_key(StorageProvisioningRole::role);
    let mut expected_roles = StorageRole::PHYSICAL;
    expected_roles.sort_unstable();
    let actual_roles = roles
        .iter()
        .map(StorageProvisioningRole::role)
        .collect::<Vec<_>>();
    if roles.len() != EXPECTED_ROLE_COUNT || actual_roles != expected_roles {
        return Err(HostError::Journal);
    }

    Ok(StorageProvisioningPlan {
        reservation_id,
        operation_id: context.operation_id,
        scale_set_id: context.scale_set_id,
        engine_id: context.engine_id.clone(),
        docker_root_digest: context.docker_root_digest.clone(),
        profile_generation_id: context.profile_generation_id.clone(),
        profile_digest: context.profile_digest.clone(),
        sample_generation_id: context.sample_generation_id.clone(),
        roles,
    })
}

fn decode_and_validate_role(
    row: &turso::Row,
    profile: &super::super::storage_budget::StorageProfile,
    pools: &BTreeMap<String, validation::PoolRow>,
) -> Result<StorageProvisioningRole, HostError> {
    let role = StorageRole::parse(&row.get::<String>(0).map_err(|_| HostError::Journal)?)?;
    if !StorageRole::PHYSICAL.contains(&role) {
        return Err(HostError::Journal);
    }
    let pool_id: String = row.get(1).map_err(|_| HostError::Journal)?;
    let medium = StorageMedium::parse(&row.get::<String>(2).map_err(|_| HostError::Journal)?)?;
    let filesystem_kind =
        StorageFilesystemKind::parse(&row.get::<String>(3).map_err(|_| HostError::Journal)?)?;
    let inode_policy =
        StorageInodePolicy::parse(&row.get::<String>(4).map_err(|_| HostError::Journal)?)?;
    let planned_filesystem_instance_digest: Option<String> =
        row.get(5).map_err(|_| HostError::Journal)?;
    let charge_bytes = positive_u64(row.get(6).map_err(|_| HostError::Journal)?)?;
    let charge_inodes = positive_u64(row.get(7).map_err(|_| HostError::Journal)?)?;
    let retained_memory_bytes = nonnegative_u64(row.get(8).map_err(|_| HostError::Journal)?)?;
    let charge = StorageProvisioningRole {
        role,
        pool_id: pool_id.clone(),
        medium,
        filesystem_kind,
        inode_policy,
        planned_filesystem_instance_digest,
        charge_bytes,
        charge_inodes,
        retained_memory_bytes,
    };
    let pool = pools.get(&pool_id).ok_or(HostError::Journal)?;
    validate_role(profile, &charge, pool)?;
    Ok(charge)
}

async fn validate_reservation_identity(
    connection: &turso::Connection,
    reservation_id: i64,
    context: &HoldContext,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT r.engine_id, r.docker_root_digest, p.engine_id, p.docker_root_digest, p.profile_generation_id, p.profile_digest, p.sample_generation_id FROM capacity_reservations AS r JOIN capacity_reservation_provisioning AS p ON p.reservation_id = r.id WHERE r.id = ?1 AND r.intent_id = ?2",
            (reservation_id, context.operation_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows.next().await.map_err(|_| HostError::Journal)?;
    let Some(row) = row else {
        return Err(HostError::Journal);
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || row.get::<String>(0).map_err(|_| HostError::Journal)? != context.engine_id
        || row.get::<String>(1).map_err(|_| HostError::Journal)? != context.docker_root_digest
        || row.get::<String>(2).map_err(|_| HostError::Journal)? != context.engine_id
        || row.get::<String>(3).map_err(|_| HostError::Journal)? != context.docker_root_digest
        || row.get::<String>(4).map_err(|_| HostError::Journal)? != context.profile_generation_id
        || row.get::<String>(5).map_err(|_| HostError::Journal)? != context.profile_digest
        || row.get::<String>(6).map_err(|_| HostError::Journal)? != context.sample_generation_id
    {
        return Err(HostError::Journal);
    }
    Ok(())
}
