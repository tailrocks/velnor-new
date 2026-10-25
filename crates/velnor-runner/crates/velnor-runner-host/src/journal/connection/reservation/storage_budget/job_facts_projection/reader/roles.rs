use std::collections::BTreeMap;

use crate::error::HostError;
use crate::journal::{StorageObserver, StorageProfile, StorageRole, StorageRoleLimit};

use super::super::model;
use super::JobFactsReadContext;
use super::attachments::{Attachment, read_attachment};
use super::values::nonnegative;

pub(super) struct StorageFacts {
    pub(super) roles: Vec<model::StorageRole>,
    pub(super) cache_live: StorageRoleLimit,
    pub(super) cache_stage: StorageRoleLimit,
}

pub(super) async fn read_storage_roles(
    connection: &turso::Connection,
    context: &JobFactsReadContext<'_>,
    profile: &StorageProfile,
) -> Result<StorageFacts, HostError> {
    let roles = read_physical_roles(connection, context).await?;
    if roles.len() != StorageRole::PHYSICAL.len() {
        return Err(HostError::Journal);
    }
    let work = roles
        .get(&StorageRole::RunnerWork)
        .ok_or(HostError::Journal)?;
    let cache_live = profile.role_limit(StorageRole::CacheLive)?;
    let cache_stage = profile.role_limit(StorageRole::CacheStage)?;
    if cache_live.alias_of() != Some(StorageRole::RunnerWork)
        || cache_stage.alias_of() != Some(StorageRole::RunnerWork)
        || cache_live.pool_id() != work.pool_id
        || cache_stage.pool_id() != work.pool_id
    {
        return Err(HostError::Journal);
    }
    let output = build_role_aliases(&roles, profile)?;
    Ok(StorageFacts {
        roles: output,
        cache_live: cache_live.clone(),
        cache_stage: cache_stage.clone(),
    })
}

async fn read_physical_roles(
    connection: &turso::Connection,
    context: &JobFactsReadContext<'_>,
) -> Result<BTreeMap<StorageRole, model::StorageRole>, HostError> {
    let mut roles = BTreeMap::new();
    let mut role_rows = connection.query(
        "SELECT role, pool_id, filesystem_instance_digest, medium, filesystem_kind, total_bytes, free_bytes, available_bytes, inode_total, inode_free, inode_available, inode_policy, owned_usage_bytes, owned_usage_inodes, charge_bytes, charge_inodes, retained_memory_bytes FROM capacity_reservation_bound_storage_roles WHERE reservation_id = ?1 ORDER BY role",
        [context.reservation_id],
    ).await.map_err(|_| HostError::Journal)?;
    while let Some(row) = role_rows.next().await.map_err(|_| HostError::Journal)? {
        let raw = read_physical_role(connection, context, &row).await?;
        if roles.insert(raw.0, raw.1).is_some() {
            return Err(HostError::Journal);
        }
    }
    Ok(roles)
}

async fn read_physical_role(
    connection: &turso::Connection,
    context: &JobFactsReadContext<'_>,
    row: &turso::Row,
) -> Result<(StorageRole, model::StorageRole), HostError> {
    let role_name: String = row.get(0).map_err(|_| HostError::Journal)?;
    let role = StorageRole::parse(&role_name)?;
    if !StorageRole::PHYSICAL.contains(&role) {
        return Err(HostError::Journal);
    }
    let contract = role.contract().ok_or(HostError::Journal)?;
    let observer = observer_name(contract.observer);
    let container_id = observer_container(contract.observer, context);
    let attachment = read_attachment(
        connection,
        context.reservation_id,
        &role_name,
        observer,
        container_id,
        contract.target,
    )
    .await?;
    Ok((
        role,
        storage_role_from_row(row, &role_name, contract.target, &attachment)?,
    ))
}

fn observer_name(observer: StorageObserver) -> &'static str {
    match observer {
        StorageObserver::Runner => "runner",
        StorageObserver::Dind => "dind",
        StorageObserver::Probe => "probe",
    }
}

fn observer_container<'a>(
    observer: StorageObserver,
    context: &'a JobFactsReadContext<'_>,
) -> Option<&'a str> {
    match observer {
        StorageObserver::Runner => Some(context.runner_container_id),
        StorageObserver::Dind => Some(context.dind_container_id),
        StorageObserver::Probe => None,
    }
}

fn storage_role_from_row(
    row: &turso::Row,
    role: &str,
    target: &str,
    attachment: &Attachment,
) -> Result<model::StorageRole, HostError> {
    let optional = |index| -> Result<Option<u64>, HostError> {
        row.get::<Option<i64>>(index)
            .map_err(|_| HostError::Journal)?
            .map(nonnegative)
            .transpose()
    };
    Ok(model::storage_role(model::StorageRoleParts {
        role: role.to_owned(),
        pool_id: row.get(1).map_err(|_| HostError::Journal)?,
        filesystem_instance_digest: row.get(2).map_err(|_| HostError::Journal)?,
        medium: row.get(3).map_err(|_| HostError::Journal)?,
        filesystem_kind: row.get(4).map_err(|_| HostError::Journal)?,
        mount_target: target.to_owned(),
        mount_id: attachment.mount_id,
        device: attachment.device,
        statx_device: attachment.statx_device,
        fs_type_magic: attachment.fs_type_magic,
        filesystem_id: attachment.filesystem_id,
        total_bytes: nonnegative(row.get(5).map_err(|_| HostError::Journal)?)?,
        free_bytes: nonnegative(row.get(6).map_err(|_| HostError::Journal)?)?,
        available_bytes: nonnegative(row.get(7).map_err(|_| HostError::Journal)?)?,
        inode_total: optional(8)?,
        inode_free: optional(9)?,
        inode_available: optional(10)?,
        inode_policy: row.get(11).map_err(|_| HostError::Journal)?,
        charge_bytes: nonnegative(row.get(14).map_err(|_| HostError::Journal)?)?,
        charge_inodes: nonnegative(row.get(15).map_err(|_| HostError::Journal)?)?,
        retained_memory_bytes: nonnegative(row.get(16).map_err(|_| HostError::Journal)?)?,
        owned_usage_bytes: nonnegative(row.get(12).map_err(|_| HostError::Journal)?)?,
        owned_usage_inodes: nonnegative(row.get(13).map_err(|_| HostError::Journal)?)?,
    }))
}

fn build_role_aliases(
    roles: &BTreeMap<StorageRole, model::StorageRole>,
    profile: &StorageProfile,
) -> Result<Vec<model::StorageRole>, HostError> {
    let work = roles
        .get(&StorageRole::RunnerWork)
        .ok_or(HostError::Journal)?;
    let mut output = Vec::with_capacity(StorageRole::ALL.len());
    for role in StorageRole::ALL {
        if let Some(raw) = roles.get(&role) {
            let _limit = profile.role_limit(role)?;
            output.push(raw.clone());
        } else if matches!(role, StorageRole::CacheLive | StorageRole::CacheStage) {
            output.push(alias_role(role, work, profile.role_limit(role)?));
        } else {
            return Err(HostError::Journal);
        }
    }
    Ok(output)
}

fn alias_role(
    role: StorageRole,
    work: &model::StorageRole,
    limit: &StorageRoleLimit,
) -> model::StorageRole {
    model::storage_role(model::StorageRoleParts {
        role: role.as_str().to_owned(),
        pool_id: work.pool_id.clone(),
        filesystem_instance_digest: work.filesystem_instance_digest.clone(),
        medium: work.medium.clone(),
        filesystem_kind: work.filesystem_kind.clone(),
        mount_target: work.mount_target.clone(),
        mount_id: work.mount_id,
        device: work.device(),
        statx_device: work.statx_device(),
        fs_type_magic: work.fs_type_magic,
        filesystem_id: work.filesystem_id,
        total_bytes: work.total_bytes,
        free_bytes: work.free_bytes,
        available_bytes: work.available_bytes,
        inode_total: work.inode_total,
        inode_free: work.inode_free,
        inode_available: work.inode_available,
        inode_policy: work.inode_policy.clone(),
        charge_bytes: limit.max_bytes(),
        charge_inodes: limit.max_inodes(),
        retained_memory_bytes: limit.retained_memory_bytes(),
        owned_usage_bytes: work.owned_usage_bytes,
        owned_usage_inodes: work.owned_usage_inodes,
    })
}
