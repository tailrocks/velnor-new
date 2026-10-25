use crate::error::HostError;
use crate::journal::{
    StorageFilesystemKind, StorageInodePolicy, StorageMedium, StorageObserver, StorageRole,
};
use crate::launch::bootstrap::BootstrapContainerRole;
use velnor_resource_probe::cleanup_observer_wire::CleanupObserverBinding;

use super::{ObservedStorageAttachment, VerifiedRootBackingMapping};

/// One filesystem capacity reservation, charged once for all its mounts.
pub(crate) struct ObservedStorageRole {
    pub(super) role: StorageRole,
    pub(super) pool_id: String,
    pub(super) filesystem_instance_digest: String,
    pub(super) medium: StorageMedium,
    pub(super) filesystem_kind: StorageFilesystemKind,
    pub(super) total_bytes: u64,
    pub(super) free_bytes: u64,
    pub(super) available_bytes: u64,
    pub(super) inode_total: Option<u64>,
    pub(super) inode_free: Option<u64>,
    pub(super) inode_available: Option<u64>,
    pub(super) inode_policy: StorageInodePolicy,
    pub(super) charge_bytes: u64,
    pub(super) charge_inodes: u64,
    pub(super) retained_memory_bytes: u64,
    pub(super) owned_usage_bytes: u64,
    pub(super) owned_usage_inodes: u64,
}

impl ObservedStorageRole {
    pub(crate) const fn role(&self) -> StorageRole {
        self.role
    }

    pub(crate) fn pool_id(&self) -> &str {
        &self.pool_id
    }

    pub(crate) fn filesystem_instance_digest(&self) -> &str {
        &self.filesystem_instance_digest
    }

    pub(crate) const fn medium(&self) -> StorageMedium {
        self.medium
    }

    pub(crate) const fn filesystem_kind(&self) -> StorageFilesystemKind {
        self.filesystem_kind
    }

    pub(crate) const fn byte_counts(&self) -> (u64, u64, u64) {
        (self.total_bytes, self.free_bytes, self.available_bytes)
    }

    pub(crate) const fn inode_counts(&self) -> (Option<u64>, Option<u64>, Option<u64>) {
        (self.inode_total, self.inode_free, self.inode_available)
    }

    pub(crate) const fn inode_policy(&self) -> StorageInodePolicy {
        self.inode_policy
    }

    pub(crate) const fn charges(&self) -> (u64, u64, u64) {
        (
            self.charge_bytes,
            self.charge_inodes,
            self.retained_memory_bytes,
        )
    }

    /// Capacity credit from measured usage on an exclusive filesystem.
    /// Shared disk roles use zero credit.
    pub(crate) const fn owned_usage(&self) -> (u64, u64) {
        (self.owned_usage_bytes, self.owned_usage_inodes)
    }
}

/// Observed resource limits for one running bootstrap container.
pub(crate) struct ObservedContainerCgroup {
    pub(super) role: BootstrapContainerRole,
    pub(super) container_id: String,
    pub(super) path: String,
    pub(super) memory_max_bytes: Option<u64>,
    pub(super) memory_current_bytes: u64,
    pub(super) cpu_quota_micros: Option<u64>,
    pub(super) cpu_period_micros: u64,
    pub(super) pids_max: Option<u64>,
}

impl ObservedContainerCgroup {
    pub(crate) const fn role(&self) -> BootstrapContainerRole {
        self.role
    }

    pub(crate) fn container_id(&self) -> &str {
        &self.container_id
    }

    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    pub(crate) const fn memory(&self) -> (Option<u64>, u64) {
        (self.memory_max_bytes, self.memory_current_bytes)
    }

    pub(crate) const fn cpu(&self) -> (Option<u64>, u64) {
        (self.cpu_quota_micros, self.cpu_period_micros)
    }

    pub(crate) const fn pids_max(&self) -> Option<u64> {
        self.pids_max
    }
}

/// Mount records and cgroup data for the capacity reservation.
pub(crate) struct VerifiedStorageResourceBinding {
    pub(super) reservation_id: i64,
    pub(super) operation_id: i64,
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
    pub(super) sample_generation_id: String,
    pub(super) profile_generation_id: String,
    pub(super) profile_digest: String,
    pub(super) runner_container_id: String,
    pub(super) dind_container_id: String,
    pub(super) instance_binding_digest: String,
    pub(super) roles: Vec<ObservedStorageRole>,
    pub(super) attachments: Vec<ObservedStorageAttachment>,
    pub(super) cgroups: Vec<ObservedContainerCgroup>,
    pub(super) root_backings: Vec<VerifiedRootBackingMapping>,
    pub(super) guest_boot_id: String,
    pub(super) cleanup_observer_binding: CleanupObserverBinding,
}

impl VerifiedStorageResourceBinding {
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

    pub(crate) fn sample_generation_id(&self) -> &str {
        &self.sample_generation_id
    }

    pub(crate) fn profile_generation_id(&self) -> &str {
        &self.profile_generation_id
    }

    pub(crate) fn profile_digest(&self) -> &str {
        &self.profile_digest
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

    pub(crate) fn roles(&self) -> &[ObservedStorageRole] {
        &self.roles
    }

    pub(crate) fn attachments(&self) -> &[ObservedStorageAttachment] {
        &self.attachments
    }

    pub(crate) fn metadata_job_facts_inode(&self) -> Result<u64, HostError> {
        let runner = unique_attachment(
            self,
            StorageRole::EngineMetadata,
            StorageObserver::Runner,
            Some(self.runner_container_id.as_str()),
            "/opt/velnor/job-facts",
        )?;
        let dind = unique_attachment(
            self,
            StorageRole::EngineMetadata,
            StorageObserver::Dind,
            Some(self.dind_container_id.as_str()),
            "/opt/velnor/job-facts",
        )?;
        if runner.volume_subpath() != Some("job-facts")
            || dind.volume_subpath() != Some("job-facts")
            || !runner.read_only()
            || !dind.read_only()
            || runner.statx_inode() == 0
            || runner.statx_inode() != dind.statx_inode()
            || runner.filesystem_instance_digest() != dind.filesystem_instance_digest()
            || runner.mount_root() != dind.mount_root()
        {
            return Err(HostError::Identity);
        }
        Ok(runner.statx_inode())
    }

    pub(crate) fn cgroups(&self) -> &[ObservedContainerCgroup] {
        &self.cgroups
    }

    pub(crate) fn root_backings(&self) -> &[VerifiedRootBackingMapping] {
        &self.root_backings
    }

    /// Return the exact observed root attachment for an owned Docker volume role.
    ///
    /// The returned row contains pre-teardown device and inode facts. `SharedRun` is
    /// checked against both worker views before either one can serve as the root row.
    pub(crate) fn cleanup_volume_root_attachment(
        &self,
        role: StorageRole,
    ) -> Result<&ObservedStorageAttachment, HostError> {
        let (observer, container_id, target) = match role {
            StorageRole::RunnerHome => (
                StorageObserver::Runner,
                Some(self.runner_container_id.as_str()),
                "/home/runner",
            ),
            StorageRole::RunnerWork => (
                StorageObserver::Runner,
                Some(self.runner_container_id.as_str()),
                "/home/runner/_work",
            ),
            StorageRole::DindData => (
                StorageObserver::Dind,
                Some(self.dind_container_id.as_str()),
                "/var/lib/docker",
            ),
            StorageRole::SharedRun => (
                StorageObserver::Runner,
                Some(self.runner_container_id.as_str()),
                "/run",
            ),
            StorageRole::EngineMetadata => (StorageObserver::Probe, None, "/velnor/docker-root"),
            _ => return Err(HostError::Identity),
        };
        let root = unique_attachment(self, role, observer, container_id, target)?;
        if root.volume_subpath().is_some() || root.statx_inode() == 0 {
            return Err(HostError::Identity);
        }
        if role == StorageRole::SharedRun {
            let dind = unique_attachment(
                self,
                role,
                StorageObserver::Dind,
                Some(self.dind_container_id.as_str()),
                target,
            )?;
            if root.source_target() != dind.source_target()
                || root.mount_root() != dind.mount_root()
                || root.device() != dind.device()
                || root.statx_device() != dind.statx_device()
                || root.statx_inode() != dind.statx_inode()
                || root.fs_type_magic() != dind.fs_type_magic()
                || root.filesystem_id() != dind.filesystem_id()
                || root.filesystem_instance_digest() != dind.filesystem_instance_digest()
            {
                return Err(HostError::Identity);
            }
        }
        Ok(root)
    }

    pub(crate) fn guest_boot_id(&self) -> &str {
        &self.guest_boot_id
    }

    /// Full pre-teardown identity required by the cleanup observer.
    pub(crate) fn cleanup_observer_binding(&self) -> &CleanupObserverBinding {
        &self.cleanup_observer_binding
    }
}

fn unique_attachment<'a>(
    binding: &'a VerifiedStorageResourceBinding,
    role: StorageRole,
    observer: StorageObserver,
    container_id: Option<&str>,
    target: &str,
) -> Result<&'a ObservedStorageAttachment, HostError> {
    let mut matches = binding.attachments.iter().filter(|attachment| {
        attachment.role() == role
            && attachment.observer() == observer
            && attachment.container_id() == container_id
            && attachment.target() == target
    });
    let attachment = matches.next().ok_or(HostError::Identity)?;
    if matches.next().is_some() {
        return Err(HostError::Identity);
    }
    Ok(attachment)
}
