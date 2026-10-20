use std::collections::BTreeMap;

use crate::error::HostError;
use crate::journal::connection::{StorageObserver, StorageRole};
use crate::launch::bootstrap::{BootstrapContainerRole, VerifiedStorageResourceBinding};

use super::super::admission::sql_i64;

const BTRFS_SUPER_MAGIC: u64 = 0x9123_683e;

pub(super) fn validate_attachments(
    binding: &VerifiedStorageResourceBinding,
    roles: &BTreeMap<StorageRole, &str>,
) -> Result<(), HostError> {
    let attachments = binding.attachments();
    let expected_count = StorageRole::PHYSICAL
        .iter()
        .map(|role| role.attachments().len())
        .sum::<usize>();
    if attachments.len() != expected_count {
        return Err(HostError::Journal);
    }
    let mut seen = BTreeMap::new();
    validate_expected_attachments(binding, roles, &mut seen)?;
    validate_shared_attachment_filesystems(&seen)?;
    Ok(())
}

fn validate_expected_attachments<'a>(
    binding: &'a VerifiedStorageResourceBinding,
    roles: &BTreeMap<StorageRole, &str>,
    seen: &mut BTreeMap<
        (StorageRole, StorageObserver, &'static str),
        &'a crate::launch::bootstrap::ObservedStorageAttachment,
    >,
) -> Result<(), HostError> {
    let attachments = binding.attachments();
    for role in StorageRole::PHYSICAL {
        for contract in role.attachments() {
            let mut matching = attachments.iter().filter(|attachment| {
                attachment.role() == contract.role
                    && attachment.observer() == contract.observer
                    && attachment.target() == contract.target
                    && attachment.read_only() == contract.read_only
                    && attachment.source_target() == contract.source_target
                    && attachment.volume_subpath() == contract.volume_subpath
            });
            let attachment = matching.next().ok_or(HostError::Journal)?;
            if matching.next().is_some() {
                return Err(HostError::Journal);
            }
            let expected_container = match contract.observer {
                StorageObserver::Runner => Some(binding.runner_container_id()),
                StorageObserver::Dind => Some(binding.dind_container_id()),
                StorageObserver::Probe => None,
            };
            let device = attachment.device();
            let statx = attachment.statx_device();
            let expected_digest = roles.get(&contract.role).ok_or(HostError::Journal)?;
            let is_root = matches!(
                contract.role,
                StorageRole::RunnerRoot | StorageRole::DindRoot
            );
            if attachment.container_id() != expected_container
                || !valid_mount_root(attachment.mount_root())
                || attachment.mount_id() == 0
                || attachment.fs_type_magic() == 0
                || attachment.filesystem_id() == 0
                || (attachment.fs_type_magic() != BTRFS_SUPER_MAGIC && device != statx)
                || !super::super::super::storage_budget::valid_digest(
                    attachment.filesystem_instance_digest(),
                )
                || (!is_root && attachment.filesystem_instance_digest() != *expected_digest)
            {
                return Err(HostError::Journal);
            }
            if seen
                .insert(
                    (contract.role, contract.observer, contract.target),
                    attachment,
                )
                .is_some()
            {
                return Err(HostError::Journal);
            }
        }
    }
    Ok(())
}

fn validate_shared_attachment_filesystems(
    seen: &BTreeMap<
        (StorageRole, StorageObserver, &'static str),
        &crate::launch::bootstrap::ObservedStorageAttachment,
    >,
) -> Result<(), HostError> {
    for role in [
        StorageRole::RunnerHome,
        StorageRole::RunnerWork,
        StorageRole::SharedRun,
    ] {
        let runner = seen
            .get(&(
                role,
                StorageObserver::Runner,
                role.contract().ok_or(HostError::Journal)?.target,
            ))
            .ok_or(HostError::Journal)?;
        let dind = seen
            .get(&(
                role,
                StorageObserver::Dind,
                role.contract().ok_or(HostError::Journal)?.target,
            ))
            .ok_or(HostError::Journal)?;
        if runner.device() != dind.device()
            || runner.statx_device() != dind.statx_device()
            || runner.fs_type_magic() != dind.fs_type_magic()
            || runner.filesystem_id() != dind.filesystem_id()
            || runner.filesystem_instance_digest() != dind.filesystem_instance_digest()
        {
            return Err(HostError::Identity);
        }
    }
    Ok(())
}

pub(super) async fn validate_cgroups(
    connection: &turso::Connection,
    reservation_id: i64,
    binding: &VerifiedStorageResourceBinding,
) -> Result<(), HostError> {
    if binding.cgroups().len() != 2 {
        return Err(HostError::Journal);
    }
    let mut rows = connection
        .query(
            "SELECT cpu_millicores, memory_bytes FROM capacity_reservations WHERE id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let cpu_limit = u64::try_from(row.get::<i64>(0).map_err(|_| HostError::Journal)?)
        .map_err(|_| HostError::Journal)?;
    let memory_limit = u64::try_from(row.get::<i64>(1).map_err(|_| HostError::Journal)?)
        .map_err(|_| HostError::Journal)?;
    let mut memory_total = 0_u64;
    let mut cpu_millicores = 0_u128;
    let mut seen_roles = Vec::with_capacity(2);
    for cgroup in binding.cgroups() {
        let expected_container = match cgroup.role() {
            BootstrapContainerRole::Runner => binding.runner_container_id(),
            BootstrapContainerRole::Dind => binding.dind_container_id(),
        };
        if cgroup.container_id() != expected_container || seen_roles.contains(&cgroup.role()) {
            return Err(HostError::Identity);
        }
        seen_roles.push(cgroup.role());
        let memory = cgroup.memory();
        let cpu = cgroup.cpu();
        let memory_max = memory.0.ok_or(HostError::Journal)?;
        let quota = cpu.0.ok_or(HostError::Journal)?;
        let pids = cgroup.pids_max().ok_or(HostError::Journal)?;
        if memory_max == 0
            || memory.1 > memory_max
            || quota == 0
            || cpu.1 == 0
            || pids == 0
            || cgroup.path().is_empty()
            || cgroup.path().len() > 1024
            || cgroup.path().bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(HostError::Journal);
        }
        memory_total = memory_total
            .checked_add(memory_max)
            .ok_or(HostError::Journal)?;
        let numerator = u128::from(quota)
            .checked_mul(1000)
            .ok_or(HostError::Journal)?;
        let denominator = u128::from(cpu.1);
        cpu_millicores = cpu_millicores
            .checked_add(numerator.div_ceil(denominator))
            .ok_or(HostError::Journal)?;
    }
    if seen_roles.len() != 2
        || !seen_roles.contains(&BootstrapContainerRole::Runner)
        || !seen_roles.contains(&BootstrapContainerRole::Dind)
        || memory_total > memory_limit
        || cpu_millicores > u128::from(cpu_limit)
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

pub(super) async fn insert_bound_rows(
    connection: &turso::Connection,
    reservation_id: i64,
    binding: &VerifiedStorageResourceBinding,
) -> Result<(), HostError> {
    for role in binding.roles() {
        let (total, free, available) = role.byte_counts();
        let (inode_total, inode_free, inode_available) = role.inode_counts();
        let (charge_bytes, charge_inodes, retained_memory) = role.charges();
        let (owned_bytes, owned_inodes) = role.owned_usage();
        connection
            .execute(
                "INSERT INTO capacity_reservation_bound_storage_roles (reservation_id, role, pool_id, filesystem_instance_digest, medium, filesystem_kind, total_bytes, free_bytes, available_bytes, inode_total, inode_free, inode_available, inode_policy, owned_usage_bytes, owned_usage_inodes, charge_bytes, charge_inodes, retained_memory_bytes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
                turso::params![reservation_id, role.role().as_str(), role.pool_id(), role.filesystem_instance_digest(), role.medium().as_str(), role.filesystem_kind().as_str(), sql_i64(total)?, sql_i64(free)?, sql_i64(available)?, sql_nullable(inode_total)?, sql_nullable(inode_free)?, sql_nullable(inode_available)?, role.inode_policy().as_str(), sql_i64(owned_bytes)?, sql_i64(owned_inodes)?, sql_i64(charge_bytes)?, sql_i64(charge_inodes)?, sql_i64(retained_memory)?],
            )
            .await
            .map_err(|_| HostError::Journal)?;
    }
    for mapping in binding.root_backings() {
        connection
            .execute(
                "INSERT INTO capacity_reservation_root_backing_mappings (reservation_id, connection_id, operation_id, engine_id, docker_root_digest, sample_generation_id, role, container_id, image_runtime_id, image_source_sha, logical_mount_id, logical_filesystem_digest, backing_filesystem_digest, mapping_record, mapping_digest) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                (mapping.reservation_id(), mapping.connection_id(), mapping.operation_id(), mapping.engine_id(), mapping.docker_root_digest(), mapping.sample_generation_id(), super::root_backing::root_backing_role(mapping.role())?, mapping.container_id(), mapping.image_runtime_id(), mapping.image_source_sha(), sql_i64(mapping.logical_mount_id())?, mapping.logical_filesystem_digest(), mapping.backing_filesystem_digest(), mapping.canonical_record(), mapping.mapping_digest()),
            )
            .await
            .map_err(|_| HostError::Journal)?;
    }
    for attachment in binding.attachments() {
        let device = attachment.device();
        let statx = attachment.statx_device();
        let filesystem_id = format!("{:016x}", attachment.filesystem_id());
        connection
            .execute(
                "INSERT INTO capacity_reservation_bound_storage_attachments (reservation_id, role, observer, container_id, mount_target, source_target, read_only, mount_id, device_major, device_minor, statx_device_major, statx_device_minor, fs_type_magic, v16_filesystem_id_0, v16_filesystem_id_1, filesystem_id, filesystem_instance_digest) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, NULL, NULL, ?14, ?15)",
                turso::params![reservation_id, attachment.role().as_str(), observer_text(attachment.observer()), attachment.container_id(), attachment.target(), attachment.source_target(), i64::from(attachment.read_only()), sql_i64(attachment.mount_id())?, i64::from(device.0), i64::from(device.1), i64::from(statx.0), i64::from(statx.1), sql_i64(attachment.fs_type_magic())?, filesystem_id, attachment.filesystem_instance_digest()],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        connection
            .execute(
                "INSERT INTO capacity_reservation_bound_storage_paths (reservation_id, role, observer, mount_target, mount_root, volume_subpath) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                (reservation_id, attachment.role().as_str(), observer_text(attachment.observer()), attachment.target(), attachment.mount_root(), attachment.volume_subpath()),
            )
            .await
            .map_err(|_| HostError::Journal)?;
    }
    for cgroup in binding.cgroups() {
        let memory = cgroup.memory();
        let cpu = cgroup.cpu();
        connection
            .execute(
                "INSERT INTO capacity_reservation_bound_cgroups (reservation_id, role, container_id, path, memory_max_bytes, memory_current_bytes, cpu_quota_micros, cpu_period_micros, pids_max) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                turso::params![reservation_id, container_role_text(cgroup.role()), cgroup.container_id(), cgroup.path(), sql_i64(memory.0.ok_or(HostError::Journal)?)?, sql_i64(memory.1)?, sql_i64(cpu.0.ok_or(HostError::Journal)?)?, sql_i64(cpu.1)?, sql_i64(cgroup.pids_max().ok_or(HostError::Journal)?)?],
            )
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

fn observer_text(observer: crate::journal::StorageObserver) -> &'static str {
    match observer {
        crate::journal::StorageObserver::Runner => "runner",
        crate::journal::StorageObserver::Dind => "dind",
        crate::journal::StorageObserver::Probe => "probe",
    }
}

fn container_role_text(role: BootstrapContainerRole) -> &'static str {
    match role {
        BootstrapContainerRole::Runner => "runner",
        BootstrapContainerRole::Dind => "dind",
    }
}

fn sql_nullable(value: Option<u64>) -> Result<turso::Value, HostError> {
    match value {
        Some(value) => Ok(turso::Value::Integer(sql_i64(value)?)),
        None => Ok(turso::Value::Null),
    }
}

fn valid_mount_root(value: &str) -> bool {
    value.starts_with('/')
        && value.len() <= 512
        && !value.chars().any(char::is_control)
        && !value.contains("//")
        && value.split('/').all(|part| part != ".." && part != ".")
}
