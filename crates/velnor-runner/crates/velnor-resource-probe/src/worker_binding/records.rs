//! Worker observation records.

use serde::{Deserialize, Serialize};

use super::{AttachmentRole, WorkerRole};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Observed filesystem attachment facts.
pub struct StorageAttachmentObservation {
    /// Fixed role for this observation.
    pub role: AttachmentRole,
    /// Absolute path inside the worker container.
    pub target: String,
    /// Read-only state from mount information.
    pub read_only: bool,
    /// Execution state from mount information.
    pub executable_mount: bool,
    /// Mount ID from mount information.
    pub mount_id: u64,
    /// Mount ID from statx.
    pub statx_mount_id: u64,
    /// Inode from statx on the observed attachment descriptor.
    pub statx_inode: u64,
    /// Mount root from mount information.
    pub mount_root: String,
    /// Mount source from mount information.
    pub mount_source: String,
    /// Device major value from mount information.
    pub mount_device_major: u32,
    /// Device minor value from mount information.
    pub mount_device_minor: u32,
    /// Device major value from statx.
    pub statx_device_major: u32,
    /// Device minor value from statx.
    pub statx_device_minor: u32,
    /// Filesystem magic value from statfs.
    pub filesystem_type_magic: u64,
    /// Filesystem name from mount information.
    pub filesystem_type_name: String,
    /// Filesystem ID from statvfs.
    pub filesystem_id: u64,
    /// Filesystem fragment size in bytes.
    pub block_size: u64,
    /// Total filesystem bytes.
    pub total_bytes: u64,
    /// Free filesystem bytes.
    pub free_bytes: u64,
    /// Unprivileged available filesystem bytes.
    pub available_bytes: u64,
    /// Total filesystem inodes.
    pub inode_total: u64,
    /// Free filesystem inodes.
    pub inode_free: u64,
    /// Unprivileged available filesystem inodes.
    pub inode_available: u64,
    /// Policy for inode capacity.
    pub inode_policy: crate::filesystem::InodePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Measured limits for the helper cgroup.
pub struct SelfLeafLimits {
    /// Actual cgroup path.
    pub cgroup_path: String,
    /// Memory limit, or null when unlimited.
    pub memory_max_bytes: Option<u64>,
    /// Current memory use in bytes.
    pub memory_current_bytes: u64,
    /// Swap limit, or null when unlimited.
    pub swap_max_bytes: Option<u64>,
    /// Current swap use in bytes.
    pub swap_current_bytes: u64,
    /// CPU quota, or null when unlimited.
    pub cpu_quota_micros: Option<u64>,
    /// CPU quota period in microseconds.
    pub cpu_period_micros: u64,
    /// Effective CPU IDs.
    pub cpuset_cpus: Vec<u32>,
    /// Process limit, or null when unlimited.
    pub pids_max: Option<u64>,
    /// Current process count.
    pub pids_current: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// One worker self storage record.
pub struct SelfStorageObservation {
    /// Producer mode name.
    pub wire_mode: String,
    /// Wire schema version. Must equal one.
    pub schema_version: u8,
    /// Fresh 32-character lower-case hexadecimal identity.
    pub operation_id: String,
    /// Exact container identity.
    pub container_id: String,
    /// Fixed role for this observation.
    pub role: WorkerRole,
    /// Helper protocol version.
    pub helper_protocol_version: u16,
    /// Source revision embedded at build time.
    pub helper_source_revision: String,
    /// Linux guest boot UUID.
    pub guest_boot_id: String,
    /// Helper PID in its own namespace.
    pub process_pid: u32,
    /// Helper start time from procfs.
    pub process_start_ticks: u64,
    /// Init start time from procfs.
    pub init_start_ticks: u64,
    /// Mount namespace inode number.
    pub mount_namespace_id: u64,
    /// Mount namespace device major value from nsfs.
    pub mount_namespace_device_major: u32,
    /// Mount namespace device minor value from nsfs.
    pub mount_namespace_device_minor: u32,
    /// Cgroup namespace inode number.
    pub cgroup_namespace_id: u64,
    /// Helper cgroup path in its namespace.
    pub cgroup_path_in_self_namespace: String,
    /// Measured limits for the helper cgroup.
    pub self_leaf_limits: SelfLeafLimits,
    /// Complete fixed attachment inventory.
    pub attachments: Vec<StorageAttachmentObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Observed process identity and membership.
pub struct ProcessWitness {
    /// Host-visible process ID.
    pub host_pid: u32,
    /// Process start time from procfs.
    pub start_ticks: u64,
    /// Mount namespace inode number.
    pub mount_namespace_id: u64,
    /// Mount namespace device major value from nsfs.
    pub mount_namespace_device_major: u32,
    /// Mount namespace device minor value from nsfs.
    pub mount_namespace_device_minor: u32,
    /// PID values from procfs status.
    pub namespace_pids: Vec<u32>,
    /// Actual cgroup path.
    pub cgroup_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Role of one overlay backing directory.
pub enum RootBackingKind {
    /// Writable container layer.
    Upper,
    /// Overlay work directory.
    Work,
    /// One read-only lower layer.
    Lower,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Raw filesystem identity and capacity for one backing directory.
pub struct RootBackingDirectoryObservation {
    /// Backing role.
    pub kind: RootBackingKind,
    /// Actual path from the worker mount options.
    pub path: String,
    /// Mount root from probe mount information.
    pub mount_root: String,
    /// Mount source from probe mount information.
    pub mount_source: String,
    /// Mount filesystem type from probe mount information.
    pub mount_filesystem_type: String,
    /// Mount read-only state from probe mount information.
    pub mount_read_only: bool,
    /// Mount device major value from probe mount information.
    pub mount_device_major: u32,
    /// Mount device minor value from probe mount information.
    pub mount_device_minor: u32,
    /// Mount ID from statx.
    pub statx_mount_id: u64,
    /// Device major value from statx.
    pub statx_device_major: u32,
    /// Device minor value from statx.
    pub statx_device_minor: u32,
    /// Filesystem magic value from statfs.
    pub filesystem_type_magic: u64,
    /// Filesystem ID from statvfs.
    pub filesystem_id: u64,
    /// Filesystem fragment size in bytes.
    pub block_size: u64,
    /// Total filesystem bytes.
    pub total_bytes: u64,
    /// Free filesystem bytes.
    pub free_bytes: u64,
    /// Available filesystem bytes.
    pub available_bytes: u64,
    /// Total filesystem inodes.
    pub inode_total: u64,
    /// Free filesystem inodes.
    pub inode_free: u64,
    /// Available filesystem inodes.
    pub inode_available: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Root overlay and its measured backing directories for one worker.
pub struct RootBackingObservation {
    /// Fixed worker role.
    pub role: WorkerRole,
    /// Exact Docker container identity.
    pub container_id: String,
    /// Linux guest boot UUID.
    pub guest_boot_id: String,
    /// Worker init process identity.
    pub init: ProcessWitness,
    /// Self-helper process identity.
    pub helper: ProcessWitness,
    /// Actual root mount source.
    pub mount_source: String,
    /// Root mount filesystem type.
    pub filesystem_type: String,
    /// Root mount ID from mount information.
    pub mount_id: u64,
    /// Root mount device major value.
    pub mount_device_major: u32,
    /// Root mount device minor value.
    pub mount_device_minor: u32,
    /// Root mount root path.
    pub mount_root: String,
    /// Root mount read-only state.
    pub read_only: bool,
    /// Actual overlay upper directory.
    pub upperdir: String,
    /// Actual overlay work directory.
    pub workdir: String,
    /// Actual overlay lower directories in order.
    pub lowerdirs: Vec<String>,
    /// Measurements for upper, work, and lower directories.
    pub directories: Vec<RootBackingDirectoryObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Measured values for one cgroup node.
pub struct CgroupNodeObservation {
    /// Actual cgroup path.
    pub path: String,
    /// True when this node is the hierarchy root.
    pub root: bool,
    /// Memory limit, or null when unlimited.
    pub memory_max_bytes: Option<u64>,
    /// Current memory use in bytes.
    pub memory_current_bytes: Option<u64>,
    /// Swap limit, or null when unlimited.
    pub swap_max_bytes: Option<u64>,
    /// Current swap use in bytes.
    pub swap_current_bytes: Option<u64>,
    /// CPU quota, or null when unlimited.
    pub cpu_quota_micros: Option<u64>,
    /// CPU quota period in microseconds.
    pub cpu_period_micros: Option<u64>,
    /// Effective CPU IDs.
    pub cpuset_cpus: Option<Vec<u32>>,
    /// Process limit, or null when unlimited.
    pub pids_max: Option<u64>,
    /// Current process count.
    pub pids_current: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// One worker process and cgroup record.
pub struct OwnedContainerCgroupObservation {
    /// Fixed role for this observation.
    pub role: WorkerRole,
    /// Exact Docker container identity.
    pub container_id: String,
    /// Container init process witness.
    pub init: ProcessWitness,
    /// Self-helper process witness.
    pub helper: ProcessWitness,
    /// Complete root-to-leaf cgroup chain.
    pub hierarchy: Vec<CgroupNodeObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Both worker cgroup records and engine metadata.
pub struct OwnedCgroupObservation {
    /// Producer mode name.
    pub wire_mode: String,
    /// Wire schema version. Must equal three.
    pub schema_version: u8,
    /// Fresh 32-character lower-case hexadecimal identity.
    pub operation_id: String,
    /// Linux guest boot UUID.
    pub guest_boot_id: String,
    /// Selected Docker-root observation.
    pub engine_metadata: StorageAttachmentObservation,
    /// Selected Docker-root object identity.
    pub docker_root_object: DockerRootObjectIdentity,
    /// Unique selected daemon process witness.
    pub daemon: ProcessWitness,
    /// Runner then `DinD` observations.
    pub containers: Vec<OwnedContainerCgroupObservation>,
    /// Independently measured worker root backing directories.
    pub root_backings: Vec<RootBackingObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Exact selected Docker-root object identity.
pub struct DockerRootObjectIdentity {
    /// Mount ID from statx.
    pub mount_id: u64,
    /// Device major value from statx.
    pub device_major: u32,
    /// Device minor value from statx.
    pub device_minor: u32,
    /// Inode value from statx.
    pub inode: u64,
    /// Filesystem type magic value from statfs.
    pub filesystem_type_magic: u64,
    /// Filesystem ID from statvfs.
    pub filesystem_id: u64,
}
