use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::error::HostError;
use crate::journal::connection::storage::in_transaction;
use crate::journal::connection::{
    ConnectionJournal, StorageInodePolicy, StorageMedium, StorageObserver, StorageRole,
};
use crate::launch::bootstrap::{BootstrapContainerRole, VerifiedStorageResourceBinding};

use super::admission::sql_i64;
use super::{HoldContext, hold_context};

const BTRFS_SUPER_MAGIC: u64 = 0x9123_683e;

pub(super) async fn bind(
    journal: &ConnectionJournal,
    reservation_id: i64,
    binding: &VerifiedStorageResourceBinding,
) -> Result<(), HostError> {
    if reservation_id <= 0 || binding.reservation_id() != reservation_id {
        return Err(HostError::Identity);
    }
    let connection_id = journal.connection_id().as_str().to_owned();
    in_transaction(journal.journal_ref(), async |connection| {
        journal.validate_scope(connection).await?;
        let Some(context) = hold_context(connection, &connection_id, reservation_id).await? else {
            return Err(HostError::Journal);
        };
        if !super::hold_is_claimable(&context)
            || context.operation_id != binding.operation_id()
            || context.engine_id != binding.engine_id()
            || context.docker_root_digest != binding.docker_root_digest()
            || context.profile_generation_id != binding.profile_generation_id()
            || context.profile_digest != binding.profile_digest()
            || context.sample_generation_id != binding.sample_generation_id()
        {
            return Err(HostError::Identity);
        }
        let Some(profile) = super::super::storage_budget::profile_generation(
            connection,
            &context.profile_generation_id,
        )
        .await?
        else {
            return Err(HostError::Journal);
        };
        profile.validate_provisioning_contract()?;
        validate_hold_objects(connection, reservation_id, binding).await?;
        validate_binding(
            &profile,
            &context,
            connection,
            &connection_id,
            reservation_id,
            binding,
        )
        .await?;

        insert_bound_rows(connection, reservation_id, binding).await?;
        let changed = connection
            .execute(
                "UPDATE capacity_reservation_provisioning_state SET resources_bound = 1, instance_binding_digest = ?1, guest_boot_id = ?2 WHERE reservation_id = ?3 AND bootstrap_start_state = 'started' AND runner_container_id = ?4 AND dind_container_id = ?5 AND resources_bound = 0 AND instance_binding_digest IS NULL AND jit_state = 'not_requested'",
                (
                    binding.instance_binding_digest(),
                    binding.guest_boot_id(),
                    reservation_id,
                    binding.runner_container_id(),
                    binding.dind_container_id(),
                ),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed != 1 {
            return Err(HostError::Journal);
        }
        let changed = connection
            .execute(
                "UPDATE connection_intents SET launch_phase = 'resources_bound' WHERE id = ?1 AND connection_id = ?2 AND kind = 'launch' AND state = 'pending' AND cleanup_proven = 0 AND launch_phase = 'bootstrap_started'",
                (context.operation_id, connection_id.as_str()),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed == 1 {
            Ok(())
        } else {
            Err(HostError::Journal)
        }
    })
    .await
}

pub(super) async fn resources_bound(
    connection: &turso::Connection,
    connection_id: &str,
    intent_id: i64,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = p.reservation_id JOIN capacity_reservations AS r ON r.id = p.reservation_id JOIN connection_intents AS i ON i.id = p.operation_id AND i.connection_id = r.connection_id WHERE p.operation_id = ?1 AND r.connection_id = ?2 AND r.state = 'reserved' AND i.state = 'pending' AND i.cleanup_proven = 0 AND i.launch_phase = 'resources_bound' AND s.bootstrap_start_state = 'started' AND s.resources_bound = 1 AND s.jit_state = 'not_requested' AND s.storage_cleanup_proven = 0 AND (SELECT COUNT(*) FROM capacity_reservation_root_backing_mappings AS m WHERE m.reservation_id = p.reservation_id AND m.connection_id = r.connection_id AND m.operation_id = p.operation_id AND m.role IN ('runner-root', 'dind-root')) = 2 AND (SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments AS a WHERE a.reservation_id = p.reservation_id) = 18 AND (SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments AS a WHERE a.reservation_id = p.reservation_id AND a.role = 'engine-metadata') = 4 AND EXISTS (SELECT 1 FROM capacity_reservation_bound_storage_paths AS x JOIN capacity_reservation_bound_storage_attachments AS a ON a.reservation_id = x.reservation_id AND a.role = x.role AND a.observer = x.observer AND a.mount_target = x.mount_target WHERE x.reservation_id = p.reservation_id AND x.role = 'runner-home' AND x.observer = 'runner' AND x.mount_target = '/opt/velnor/source-proof/inbox' AND x.volume_subpath IS NOT NULL AND x.volume_subpath = 'source-proof/inbox' AND a.read_only = 0) AND NOT EXISTS (SELECT 1 FROM capacity_reservation_bound_storage_paths AS x WHERE x.reservation_id = p.reservation_id AND x.role = 'engine-metadata' AND x.observer = 'runner' AND x.mount_target = '/opt/velnor/source-proof/inbox') AND (SELECT COUNT(*) FROM capacity_reservation_bound_storage_paths AS x WHERE x.reservation_id = p.reservation_id) = 18",
            (intent_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_some())
}

async fn validate_hold_objects(
    connection: &turso::Connection,
    reservation_id: i64,
    binding: &VerifiedStorageResourceBinding,
) -> Result<(), HostError> {
    let names = reservation_names(connection, reservation_id).await?;
    let mut rows = connection
        .query(
        "SELECT role, docker_name, create_state, docker_id FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1 ORDER BY role",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut objects = BTreeMap::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let role: String = row.get(0).map_err(|_| HostError::Journal)?;
        let name: String = row.get(1).map_err(|_| HostError::Journal)?;
        let state: String = row.get(2).map_err(|_| HostError::Journal)?;
        let id: Option<String> = row.get(3).map_err(|_| HostError::Journal)?;
        let expected_name = match role.as_str() {
            "shared-run" => names.volume_name(crate::worker::WorkerVolumeRole::Socket),
            "runner-home" => names.volume_name(crate::worker::WorkerVolumeRole::Home),
            "runner-work" => names.volume_name(crate::worker::WorkerVolumeRole::Work),
            "dind-data" => names.volume_name(crate::worker::WorkerVolumeRole::DindData),
            "engine-metadata" => names.volume_name(crate::worker::WorkerVolumeRole::RunnerMetadata),
            _ => return Err(HostError::Journal),
        };
        if state != "created" || name != expected_name || id.as_deref() != Some(name.as_str()) {
            return Err(HostError::Journal);
        }
        objects.insert(role, id.ok_or(HostError::Journal)?);
    }
    if objects.len() != 5 {
        return Err(HostError::Journal);
    }

    let mut rows = connection
        .query(
            "SELECT b.role, b.container_name, b.container_id, b.create_state, b.start_state, s.bootstrap_start_state, s.runner_container_id, s.dind_container_id FROM capacity_reservation_bootstrap_containers AS b JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = b.reservation_id WHERE b.reservation_id = ?1 ORDER BY b.role",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut containers = BTreeMap::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let role: String = row.get(0).map_err(|_| HostError::Journal)?;
        let name: String = row.get(1).map_err(|_| HostError::Journal)?;
        let id: Option<String> = row.get(2).map_err(|_| HostError::Journal)?;
        let create_state: String = row.get(3).map_err(|_| HostError::Journal)?;
        let start_state: String = row.get(4).map_err(|_| HostError::Journal)?;
        let bootstrap_state: String = row.get(5).map_err(|_| HostError::Journal)?;
        let runner_id: Option<String> = row.get(6).map_err(|_| HostError::Journal)?;
        let dind_id: Option<String> = row.get(7).map_err(|_| HostError::Journal)?;
        if create_state != "created" || start_state != "started" || bootstrap_state != "started" {
            return Err(HostError::Journal);
        }
        let id = id.ok_or(HostError::Journal)?;
        if match role.as_str() {
            "runner" => {
                name == names.container_name(crate::worker::WorkerContainerRole::Runner)
                    && id == binding.runner_container_id()
                    && runner_id.as_deref() == Some(id.as_str())
            }
            "dind" => {
                name == names.container_name(crate::worker::WorkerContainerRole::Dind)
                    && id == binding.dind_container_id()
                    && dind_id.as_deref() == Some(id.as_str())
            }
            _ => false,
        } {
            containers.insert(role, id);
        } else {
            return Err(HostError::Identity);
        }
    }
    if containers.len() != 2 {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn reservation_names(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<crate::worker::WorkerIdentityNames, HostError> {
    let mut rows = connection
        .query(
            "SELECT i.runner_name FROM connection_intents AS i JOIN capacity_reservations AS r ON r.intent_id = i.id AND r.connection_id = i.connection_id WHERE r.id = ?1 AND i.kind = 'launch'",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let name: String = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    let nonce = name.strip_prefix('v').ok_or(HostError::Journal)?;
    crate::worker::WorkerIdentityNames::from_nonce(nonce)
}

async fn validate_binding(
    profile: &super::super::storage_budget::StorageProfile,
    context: &HoldContext,
    connection: &turso::Connection,
    connection_id: &str,
    reservation_id: i64,
    binding: &VerifiedStorageResourceBinding,
) -> Result<(), HostError> {
    if binding.engine_id() != context.engine_id
        || binding.docker_root_digest() != context.docker_root_digest
        || binding.sample_generation_id() != context.sample_generation_id
        || binding.profile_generation_id() != context.profile_generation_id
        || binding.profile_digest() != context.profile_digest
    {
        return Err(HostError::Identity);
    }
    validate_roles(profile, connection, connection_id, reservation_id, binding).await?;
    validate_cgroups(connection, reservation_id, binding).await
}

async fn validate_roles(
    profile: &super::super::storage_budget::StorageProfile,
    connection: &turso::Connection,
    connection_id: &str,
    reservation_id: i64,
    binding: &VerifiedStorageResourceBinding,
) -> Result<(), HostError> {
    if binding.roles().len() != StorageRole::PHYSICAL.len() {
        return Err(HostError::Journal);
    }
    let mut sample_pools = BTreeMap::new();
    let mut rows = connection
        .query(
            "SELECT pool_id, scope, medium, filesystem_kind, inode_policy, filesystem_instance_digest, max_bytes, max_inodes FROM capacity_reservation_provisioning_pools WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let pool_id: String = row.get(0).map_err(|_| HostError::Journal)?;
        let scope: String = row.get(1).map_err(|_| HostError::Journal)?;
        let medium: String = row.get(2).map_err(|_| HostError::Journal)?;
        let kind: String = row.get(3).map_err(|_| HostError::Journal)?;
        let inode_policy: String = row.get(4).map_err(|_| HostError::Journal)?;
        let digest: Option<String> = row.get(5).map_err(|_| HostError::Journal)?;
        let max_bytes: i64 = row.get(6).map_err(|_| HostError::Journal)?;
        let max_inodes: i64 = row.get(7).map_err(|_| HostError::Journal)?;
        let expected = profile.pool_limit(&pool_id)?;
        if medium != expected.medium().as_str()
            || inode_policy != expected.inode_policy().as_str()
            || sql_i64(expected.max_bytes())? != max_bytes
            || sql_i64(expected.max_inodes())? != max_inodes
        {
            return Err(HostError::Journal);
        }
        sample_pools.insert(pool_id, (scope, kind, digest));
    }
    if sample_pools.len() != 9 {
        return Err(HostError::Journal);
    }

    let mut seen = BTreeMap::new();
    for observed in binding.roles() {
        let role = observed.role();
        let limit = profile.role_limit(role)?;
        let pool_limit = profile.pool_limit(limit.pool_id())?;
        let (charge_bytes, charge_inodes, retained_memory) = observed.charges();
        let (total, free, available) = observed.byte_counts();
        let (inode_total, inode_free, inode_available) = observed.inode_counts();
        let (owned_bytes, owned_inodes) = observed.owned_usage();
        let (scope, expected_kind, sample_digest) = sample_pools
            .get(limit.pool_id())
            .ok_or(HostError::Journal)?;
        if observed.pool_id() != limit.pool_id()
            || observed.medium() != limit.medium()
            || observed.filesystem_kind().as_str() != expected_kind
            || observed.inode_policy() != pool_limit.inode_policy()
            || charge_bytes != limit.max_bytes()
            || charge_inodes != limit.max_inodes()
            || retained_memory != limit.retained_memory_bytes()
            || total == 0
            || free > total
            || available > free
            || (observed.medium() == StorageMedium::Tmpfs
                && (*scope != "exclusive-tmpfs"
                    || observed.filesystem_kind().as_str() != "tmpfs"
                    || total != charge_bytes
                    || inode_total != Some(charge_inodes)
                    || available
                        .checked_add(owned_bytes)
                        .is_none_or(|value| value < charge_bytes)
                    || inode_available
                        .and_then(|value| value.checked_add(owned_inodes))
                        .is_none_or(|value| value < charge_inodes)))
            || (observed.medium() == StorageMedium::Disk
                && (*scope != "shared-disk"
                    || sample_digest.as_deref() != Some(observed.filesystem_instance_digest())
                    || owned_bytes != 0
                    || owned_inodes != 0))
        {
            return Err(HostError::Journal);
        }
        validate_observed_inode_values(
            observed.inode_policy(),
            observed.filesystem_kind().as_str(),
            inode_total,
            inode_free,
            inode_available,
        )?;
        if seen
            .insert(role, observed.filesystem_instance_digest())
            .is_some()
        {
            return Err(HostError::Journal);
        }
    }
    if seen.len() != StorageRole::PHYSICAL.len() {
        return Err(HostError::Journal);
    }
    let disk_ids = [
        StorageRole::RunnerRoot,
        StorageRole::DindRoot,
        StorageRole::EngineMetadata,
    ]
    .map(|role| seen.get(&role).copied().ok_or(HostError::Journal))
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;
    if disk_ids.windows(2).any(|pair| pair[0] != pair[1]) {
        return Err(HostError::Journal);
    }
    validate_root_backings(connection_id, reservation_id, binding, &seen)?;
    validate_attachments(binding, &seen)?;
    Ok(())
}

fn validate_root_backings(
    connection_id: &str,
    reservation_id: i64,
    binding: &VerifiedStorageResourceBinding,
    roles: &BTreeMap<StorageRole, &str>,
) -> Result<(), HostError> {
    let mappings = binding.root_backings();
    if mappings.len() != 2 {
        return Err(HostError::Journal);
    }
    let mut seen = BTreeMap::new();
    for mapping in mappings {
        let role = mapping.role();
        let (role_text, observer, container_id) = match role {
            StorageRole::RunnerRoot => (
                "runner-root",
                StorageObserver::Runner,
                binding.runner_container_id(),
            ),
            StorageRole::DindRoot => (
                "dind-root",
                StorageObserver::Dind,
                binding.dind_container_id(),
            ),
            _ => return Err(HostError::Identity),
        };
        let logical = binding
            .attachments()
            .iter()
            .find(|attachment| {
                attachment.role() == role
                    && attachment.observer() == observer
                    && attachment.target() == "/"
            })
            .ok_or(HostError::Journal)?;
        let capacity_digest = roles.get(&role).ok_or(HostError::Journal)?;
        if mapping.reservation_id() != reservation_id
            || mapping.operation_id() != binding.operation_id()
            || mapping.connection_id() != connection_id
            || mapping.engine_id() != binding.engine_id()
            || mapping.docker_root_digest() != binding.docker_root_digest()
            || mapping.sample_generation_id() != binding.sample_generation_id()
            || mapping.container_id() != container_id
            || !valid_runtime_image_id(mapping.image_runtime_id())
            || !valid_git_sha(mapping.image_source_sha())
            || mapping.logical_mount_id() != logical.mount_id()
            || mapping.logical_device() != logical.device()
            || mapping.logical_statx_device() != logical.statx_device()
            || mapping.logical_fs_type_magic() != logical.fs_type_magic()
            || mapping.logical_filesystem_id() != logical.filesystem_id()
            || mapping.logical_filesystem_digest() != logical.filesystem_instance_digest()
            || mapping.backing_filesystem_digest() != *capacity_digest
            || !super::super::storage_budget::valid_digest(mapping.logical_filesystem_digest())
            || !super::super::storage_budget::valid_digest(mapping.backing_filesystem_digest())
            || !super::super::storage_budget::valid_digest(mapping.mapping_digest())
            || sha256(mapping.canonical_record().as_bytes()) != mapping.mapping_digest()
            || seen.insert(role_text, mapping).is_some()
        {
            return Err(HostError::Identity);
        }
    }
    if seen.len() != 2 {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"velnor-root-backing-map-v1\0");
    digest.update(bytes);
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn valid_runtime_image_id(value: &str) -> bool {
    (1..=255).contains(&value.len()) && !value.chars().any(char::is_control)
}

fn valid_git_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_attachments(
    binding: &VerifiedStorageResourceBinding,
    roles: &BTreeMap<StorageRole, &str>,
) -> Result<(), HostError> {
    let attachments = binding.attachments();
    let expected_count = StorageRole::PHYSICAL
        .iter()
        .map(|role| role.attachments().len())
        .sum::<usize>();
    if expected_count != 18 || attachments.len() != 18 {
        return Err(HostError::Journal);
    }
    let mut seen = BTreeMap::new();
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
                || !super::super::storage_budget::valid_digest(
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

async fn validate_cgroups(
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

async fn insert_bound_rows(
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
                (mapping.reservation_id(), mapping.connection_id(), mapping.operation_id(), mapping.engine_id(), mapping.docker_root_digest(), mapping.sample_generation_id(), root_backing_role(mapping.role())?, mapping.container_id(), mapping.image_runtime_id(), mapping.image_source_sha(), sql_i64(mapping.logical_mount_id())?, mapping.logical_filesystem_digest(), mapping.backing_filesystem_digest(), mapping.canonical_record(), mapping.mapping_digest()),
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

fn root_backing_role(role: StorageRole) -> Result<&'static str, HostError> {
    match role {
        StorageRole::RunnerRoot => Ok("runner-root"),
        StorageRole::DindRoot => Ok("dind-root"),
        _ => Err(HostError::Identity),
    }
}

fn validate_observed_inode_values(
    policy: StorageInodePolicy,
    filesystem_kind: &str,
    total: Option<u64>,
    free: Option<u64>,
    available: Option<u64>,
) -> Result<(), HostError> {
    match (policy, total, free, available) {
        (StorageInodePolicy::Finite, Some(total), Some(free), Some(available))
            if total > 0 && free <= total && available <= free =>
        {
            Ok(())
        }
        (StorageInodePolicy::DynamicBtrfs, Some(0), Some(0), Some(0))
            if filesystem_kind == "btrfs" =>
        {
            Ok(())
        }
        _ => Err(HostError::Journal),
    }
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
