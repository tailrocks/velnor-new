use crate::journal::{
    StorageFilesystemKind, StorageInodePolicy, StorageMedium, StorageObserver, StorageRole,
};

/// One physical filesystem entitlement, charged once regardless of mounts.
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

    /// Initialized bytes and objects within this dedicated owned filesystem.
    pub(crate) const fn owned_usage(&self) -> (u64, u64) {
        (self.owned_usage_bytes, self.owned_usage_inodes)
    }
}

/// Mount-level witness. Shared tmpfs volumes have one physical role row and
/// separate Runner/DinD attachments with the same backing filesystem digest.
pub(crate) struct ObservedStorageAttachment {
    role: StorageRole,
    observer: StorageObserver,
    container_id: Option<String>,
    target: String,
    source_target: Option<String>,
    read_only: bool,
    mount_id: u64,
    device_major: u32,
    device_minor: u32,
    statx_device_major: u32,
    statx_device_minor: u32,
    fs_type_magic: u64,
    filesystem_id: [i64; 2],
    filesystem_instance_digest: String,
}

impl ObservedStorageAttachment {
    pub(crate) const fn role(&self) -> StorageRole {
        self.role
    }

    pub(crate) const fn observer(&self) -> StorageObserver {
        self.observer
    }

    pub(crate) fn container_id(&self) -> Option<&str> {
        self.container_id.as_deref()
    }

    pub(crate) fn target(&self) -> &str {
        &self.target
    }

    pub(crate) fn source_target(&self) -> Option<&str> {
        self.source_target.as_deref()
    }

    pub(crate) const fn read_only(&self) -> bool {
        self.read_only
    }

    pub(crate) const fn mount_id(&self) -> u64 {
        self.mount_id
    }

    pub(crate) const fn device(&self) -> (u32, u32) {
        (self.device_major, self.device_minor)
    }

    pub(crate) const fn statx_device(&self) -> (u32, u32) {
        (self.statx_device_major, self.statx_device_minor)
    }

    pub(crate) const fn fs_type_magic(&self) -> u64 {
        self.fs_type_magic
    }

    pub(crate) const fn filesystem_id(&self) -> [i64; 2] {
        self.filesystem_id
    }

    pub(crate) fn filesystem_instance_digest(&self) -> &str {
        &self.filesystem_instance_digest
    }
}

/// Observed resource limits for one already started bootstrap container.
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

/// Actual mounts and cgroups bound to the durable provisioning hold.
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

    pub(crate) fn guest_boot_id(&self) -> &str {
        &self.guest_boot_id
    }
}

mod validation;

#[cfg(test)]
#[path = "test_fixtures/storage.rs"]
mod test_fixtures;

#[cfg(test)]
pub(crate) use test_fixtures::{
    test_observed_container_cgroup, test_observed_storage_attachment, test_observed_storage_role,
    test_verified_storage_resource_binding,
};
