use crate::error::HostError;
use crate::journal::connection::storage::read_transaction;
use crate::journal::connection::{ConnectionJournal, StorageRole};

use super::{bound_identity, cleanup_inventory, hold_context, names_for_reservation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StorageCleanupIdentity {
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    runner_container_id: String,
    dind_container_id: String,
    instance_binding_digest: String,
    topology_version: i64,
    private_volume: String,
    guest_boot_id: String,
    volumes: Vec<(StorageRole, String)>,
    mounts: Vec<StorageCleanupMount>,
    cgroups: Vec<StorageCleanupCgroup>,
}

impl StorageCleanupIdentity {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn runner_container_id(&self) -> &str {
        &self.runner_container_id
    }

    pub(crate) fn dind_container_id(&self) -> &str {
        &self.dind_container_id
    }

    pub(crate) fn instance_binding_digest(&self) -> &str {
        &self.instance_binding_digest
    }

    pub(crate) const fn topology_version(&self) -> i64 {
        self.topology_version
    }

    pub(crate) fn private_volume(&self) -> &str {
        &self.private_volume
    }

    pub(crate) fn guest_boot_id(&self) -> &str {
        &self.guest_boot_id
    }

    pub(crate) fn volumes(&self) -> &[(StorageRole, String)] {
        &self.volumes
    }

    pub(crate) fn mounts(&self) -> &[StorageCleanupMount] {
        &self.mounts
    }

    pub(crate) fn cgroups(&self) -> &[StorageCleanupCgroup] {
        &self.cgroups
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StorageCleanupMount {
    role: String,
    observer: String,
    container_id: Option<String>,
    target: String,
    source: Option<String>,
    read_only: bool,
    mount_id: u64,
    device: (u32, u32),
    statx_device: (u32, u32),
    fs_type_magic: u64,
    filesystem_id: Option<String>,
    filesystem_instance_digest: String,
}

impl StorageCleanupMount {
    pub(crate) fn role(&self) -> &str {
        &self.role
    }
    pub(crate) fn observer(&self) -> &str {
        &self.observer
    }
    pub(crate) fn container_id(&self) -> Option<&str> {
        self.container_id.as_deref()
    }
    pub(crate) fn target(&self) -> &str {
        &self.target
    }
    pub(crate) fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }
    pub(crate) const fn read_only(&self) -> bool {
        self.read_only
    }
    pub(crate) const fn mount_id(&self) -> u64 {
        self.mount_id
    }
    pub(crate) const fn device(&self) -> (u32, u32) {
        self.device
    }
    pub(crate) const fn statx_device(&self) -> (u32, u32) {
        self.statx_device
    }
    pub(crate) const fn fs_type_magic(&self) -> u64 {
        self.fs_type_magic
    }
    pub(crate) fn filesystem_id(&self) -> Option<&str> {
        self.filesystem_id.as_deref()
    }
    pub(crate) fn filesystem_instance_digest(&self) -> &str {
        &self.filesystem_instance_digest
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StorageCleanupCgroup {
    role: String,
    container_id: String,
    path: String,
    memory_max_bytes: u64,
    cpu_quota_micros: u64,
    cpu_period_micros: u64,
    pids_max: u64,
}

impl StorageCleanupCgroup {
    pub(crate) fn role(&self) -> &str {
        &self.role
    }
    pub(crate) fn container_id(&self) -> &str {
        &self.container_id
    }
    pub(crate) fn path(&self) -> &str {
        &self.path
    }
    pub(crate) const fn memory_max_bytes(&self) -> u64 {
        self.memory_max_bytes
    }
    pub(crate) const fn cpu_quota_micros(&self) -> u64 {
        self.cpu_quota_micros
    }
    pub(crate) const fn cpu_period_micros(&self) -> u64 {
        self.cpu_period_micros
    }
    pub(crate) const fn pids_max(&self) -> u64 {
        self.pids_max
    }
}

impl ConnectionJournal {
    /// Read the exact owned identities for a started, bound, held reservation.
    pub(crate) async fn storage_cleanup_identity(
        &self,
        reservation_id: i64,
    ) -> Result<Option<StorageCleanupIdentity>, HostError> {
        if reservation_id <= 0 {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let Some(context) = hold_context(connection, &connection_id, reservation_id).await?
            else {
                return Ok(None);
            };
            if !matches!(context.reservation_state.as_str(), "reserved" | "uncertain")
                || !matches!(
                    context.intent_state.as_str(),
                    "pending" | "uncertain" | "done"
                )
                || context.cleanup_proven
            {
                return Ok(None);
            }
            let Some(guest_boot_id) = read_boot_id(connection, reservation_id).await? else {
                return Ok(None);
            };
            let binding = bound_identity(connection, reservation_id).await?;
            let inventory = cleanup_inventory(connection, reservation_id).await?;
            let names = names_for_reservation(connection, reservation_id).await?;
            let private_volume = names.private_volume().to_owned();
            let roles: &[StorageRole] = match context.topology_version {
                1 => &[
                    StorageRole::SharedRun,
                    StorageRole::RunnerHome,
                    StorageRole::RunnerWork,
                    StorageRole::DindData,
                ],
                2 => &[
                    StorageRole::SharedRun,
                    StorageRole::RunnerHome,
                    StorageRole::RunnerWork,
                    StorageRole::DindData,
                    StorageRole::EngineMetadata,
                ],
                _ => return Err(HostError::Journal),
            };
            if roles.len() != inventory.volume_names.len() {
                return Err(HostError::Journal);
            }
            let volumes = roles
                .iter()
                .copied()
                .zip(inventory.volume_names)
                .collect::<Vec<_>>();
            let mounts = read_mounts(connection, reservation_id, &binding).await?;
            let cgroups = read_cgroups(connection, reservation_id, &binding).await?;
            Ok(Some(StorageCleanupIdentity {
                reservation_id,
                operation_id: context.operation_id,
                engine_id: context.engine_id,
                docker_root_digest: context.docker_root_digest,
                runner_container_id: binding.runner_id,
                dind_container_id: binding.dind_id,
                instance_binding_digest: binding.instance_binding_digest,
                topology_version: context.topology_version,
                private_volume,
                guest_boot_id,
                volumes,
                mounts,
                cgroups,
            }))
        })
        .await
    }
}

async fn read_boot_id(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<Option<String>, HostError> {
    let mut rows = connection.query(
        "SELECT guest_boot_id FROM capacity_reservation_provisioning_state WHERE reservation_id = ?1 AND bootstrap_start_state = 'started' AND resources_bound = 1 AND storage_cleanup_proven = 0",
        [reservation_id],
    ).await.map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let boot_id: String = row.get(0).map_err(|_| HostError::Journal)?;
    if boot_id.is_empty() || rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Some(boot_id))
}

async fn read_mounts(
    connection: &turso::Connection,
    reservation_id: i64,
    binding: &super::BoundIdentity,
) -> Result<Vec<StorageCleanupMount>, HostError> {
    let mut rows = connection.query(
        "SELECT role, observer, container_id, mount_target, source_target, read_only, mount_id, device_major, device_minor, statx_device_major, statx_device_minor, fs_type_magic, filesystem_id, filesystem_instance_digest FROM capacity_reservation_bound_storage_attachments WHERE reservation_id = ?1 ORDER BY role, observer, mount_target",
        [reservation_id],
    ).await.map_err(|_| HostError::Journal)?;
    let mut mounts = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let item = StorageCleanupMount {
            role: row.get(0).map_err(|_| HostError::Journal)?,
            observer: row.get(1).map_err(|_| HostError::Journal)?,
            container_id: row.get(2).map_err(|_| HostError::Journal)?,
            target: row.get(3).map_err(|_| HostError::Journal)?,
            source: row.get(4).map_err(|_| HostError::Journal)?,
            read_only: row.get::<i64>(5).map_err(|_| HostError::Journal)? == 1,
            mount_id: positive(row.get(6).map_err(|_| HostError::Journal)?)?,
            device: (
                nonnegative_u32(row.get(7).map_err(|_| HostError::Journal)?)?,
                nonnegative_u32(row.get(8).map_err(|_| HostError::Journal)?)?,
            ),
            statx_device: (
                nonnegative_u32(row.get(9).map_err(|_| HostError::Journal)?)?,
                nonnegative_u32(row.get(10).map_err(|_| HostError::Journal)?)?,
            ),
            fs_type_magic: nonnegative(row.get(11).map_err(|_| HostError::Journal)?)?,
            filesystem_id: row.get(12).map_err(|_| HostError::Journal)?,
            filesystem_instance_digest: row.get(13).map_err(|_| HostError::Journal)?,
        };
        if item.observer == "runner"
            && item.container_id.as_deref() != Some(binding.runner_id.as_str())
            || item.observer == "dind"
                && item.container_id.as_deref() != Some(binding.dind_id.as_str())
        {
            return Err(HostError::Journal);
        }
        mounts.push(item);
    }
    if mounts.len() < 14 || mounts.len() > 16 {
        return Err(HostError::Journal);
    }
    Ok(mounts)
}

async fn read_cgroups(
    connection: &turso::Connection,
    reservation_id: i64,
    binding: &super::BoundIdentity,
) -> Result<Vec<StorageCleanupCgroup>, HostError> {
    let mut rows = connection.query(
        "SELECT role, container_id, path, memory_max_bytes, cpu_quota_micros, cpu_period_micros, pids_max FROM capacity_reservation_bound_cgroups WHERE reservation_id = ?1 ORDER BY role",
        [reservation_id],
    ).await.map_err(|_| HostError::Journal)?;
    let mut values = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let value = StorageCleanupCgroup {
            role: row.get(0).map_err(|_| HostError::Journal)?,
            container_id: row.get(1).map_err(|_| HostError::Journal)?,
            path: row.get(2).map_err(|_| HostError::Journal)?,
            memory_max_bytes: positive(row.get(3).map_err(|_| HostError::Journal)?)?,
            cpu_quota_micros: positive(row.get(4).map_err(|_| HostError::Journal)?)?,
            cpu_period_micros: positive(row.get(5).map_err(|_| HostError::Journal)?)?,
            pids_max: positive(row.get(6).map_err(|_| HostError::Journal)?)?,
        };
        if (value.role == "runner" && value.container_id != binding.runner_id)
            || (value.role == "dind" && value.container_id != binding.dind_id)
            || !matches!(value.role.as_str(), "runner" | "dind")
        {
            return Err(HostError::Journal);
        }
        values.push(value);
    }
    if values.len() != 2 {
        return Err(HostError::Journal);
    }
    Ok(values)
}

fn positive(value: i64) -> Result<u64, HostError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(HostError::Journal)
}

fn nonnegative(value: i64) -> Result<u64, HostError> {
    u64::try_from(value).map_err(|_| HostError::Journal)
}

fn nonnegative_u32(value: i64) -> Result<u32, HostError> {
    u32::try_from(value).map_err(|_| HostError::Journal)
}
