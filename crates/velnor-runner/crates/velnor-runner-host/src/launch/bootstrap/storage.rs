mod attachment;
mod backing;
pub(crate) use attachment::ObservedStorageAttachment;
pub(crate) use backing::VerifiedRootBackingMapping;

use crate::{
    error::HostError,
    journal::{
        StorageFilesystemKind, StorageInodePolicy, StorageMedium, StorageObserver, StorageRole,
    },
};

/// One filesystem capacity reservation, charged once for all its mounts.
pub(crate) struct ObservedStorageRole {
    role: StorageRole,
    pool_id: String,
    filesystem_instance_digest: String,
    medium: StorageMedium,
    filesystem_kind: StorageFilesystemKind,
    total_bytes: u64,
    free_bytes: u64,
    available_bytes: u64,
    inode_total: Option<u64>,
    inode_free: Option<u64>,
    inode_available: Option<u64>,
    inode_policy: StorageInodePolicy,
    charge_bytes: u64,
    charge_inodes: u64,
    retained_memory_bytes: u64,
    owned_usage_bytes: u64,
    owned_usage_inodes: u64,
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
    role: super::BootstrapContainerRole,
    container_id: String,
    path: String,
    memory_max_bytes: Option<u64>,
    memory_current_bytes: u64,
    cpu_quota_micros: Option<u64>,
    cpu_period_micros: u64,
    pids_max: Option<u64>,
}

impl ObservedContainerCgroup {
    pub(crate) const fn role(&self) -> super::BootstrapContainerRole {
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
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    sample_generation_id: String,
    profile_generation_id: String,
    profile_digest: String,
    runner_container_id: String,
    dind_container_id: String,
    instance_binding_digest: String,
    roles: Vec<ObservedStorageRole>,
    attachments: Vec<ObservedStorageAttachment>,
    cgroups: Vec<ObservedContainerCgroup>,
    root_backings: Vec<VerifiedRootBackingMapping>,
    guest_boot_id: String,
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

mod observation;
mod production;
mod validation;

pub(crate) use observation::observe_storage_resources;

#[cfg(test)]
mod tests {
    use super::*;

    fn attachment(
        role: StorageRole,
        observer: StorageObserver,
        container_id: Option<&str>,
        target: &str,
        inode: u64,
    ) -> ObservedStorageAttachment {
        ObservedStorageAttachment {
            role,
            observer,
            container_id: container_id.map(str::to_owned),
            target: target.to_owned(),
            source_target: None,
            volume_subpath: None,
            mount_root: "/".to_owned(),
            read_only: false,
            mount_id: 12,
            statx_inode: inode,
            device_major: 0,
            device_minor: 41,
            statx_device_major: 0,
            statx_device_minor: 41,
            fs_type_magic: 0x0102_1994,
            filesystem_id: 7,
            filesystem_instance_digest: "a".repeat(64),
        }
    }

    fn binding(attachments: Vec<ObservedStorageAttachment>) -> VerifiedStorageResourceBinding {
        VerifiedStorageResourceBinding {
            reservation_id: 1,
            operation_id: 2,
            engine_id: "engine".to_owned(),
            docker_root_digest: "b".repeat(64),
            sample_generation_id: "c".repeat(32),
            profile_generation_id: "d".repeat(32),
            profile_digest: "e".repeat(64),
            runner_container_id: "f".repeat(64),
            dind_container_id: "1".repeat(64),
            instance_binding_digest: "2".repeat(64),
            roles: Vec::new(),
            attachments,
            cgroups: Vec::new(),
            root_backings: Vec::new(),
            guest_boot_id: "g".repeat(36),
        }
    }

    #[test]
    fn cleanup_root_accessor_returns_exact_home_root_inode() {
        let row = attachment(
            StorageRole::RunnerHome,
            StorageObserver::Runner,
            Some(&"f".repeat(64)),
            "/home/runner",
            73,
        );
        let binding = binding(vec![row]);
        let root = binding
            .cleanup_volume_root_attachment(StorageRole::RunnerHome)
            .expect("exact Home volume root");
        assert_eq!(root.target(), "/home/runner");
        assert_eq!(root.statx_inode(), 73);
        assert_eq!(root.device(), (0, 41));
    }

    #[test]
    fn cleanup_root_accessor_rejects_disagreeing_shared_run_views() {
        let runner = attachment(
            StorageRole::SharedRun,
            StorageObserver::Runner,
            Some(&"f".repeat(64)),
            "/run",
            73,
        );
        let dind = attachment(
            StorageRole::SharedRun,
            StorageObserver::Dind,
            Some(&"1".repeat(64)),
            "/run",
            74,
        );
        assert!(
            binding(vec![runner, dind])
                .cleanup_volume_root_attachment(StorageRole::SharedRun)
                .is_err()
        );
    }
}
pub(crate) use production::{PreparedTmpfsStorage, create_and_initialize};

#[cfg(test)]
#[path = "test_fixtures/storage.rs"]
mod test_fixtures;

#[cfg(test)]
pub(crate) use test_fixtures::{
    test_observed_container_cgroup, test_observed_storage_attachment, test_observed_storage_role,
    test_verified_storage_resource_binding,
};
