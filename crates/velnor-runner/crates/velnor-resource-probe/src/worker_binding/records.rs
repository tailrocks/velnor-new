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
    /// Mount root from mount information.
    pub mount_root: String,
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
    /// PID values from procfs status.
    pub namespace_pids: Vec<u32>,
    /// Actual cgroup path.
    pub cgroup_path: String,
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
    /// Wire schema version. Must equal one.
    pub schema_version: u8,
    /// Fresh 32-character lower-case hexadecimal identity.
    pub operation_id: String,
    /// Linux guest boot UUID.
    pub guest_boot_id: String,
    /// Selected Docker-root observation.
    pub engine_metadata: StorageAttachmentObservation,
    /// Runner then `DinD` observations.
    pub containers: Vec<OwnedContainerCgroupObservation>,
}
