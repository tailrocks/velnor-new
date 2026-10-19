//! Records shared by the macOS controller and the Linux guest observer.

use serde::{Deserialize, Serialize};

/// Identity data from the controller, selected daemon, and worker containers for one cleanup inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupObserverBinding {
    /// Guest boot identity recorded by the controller.
    pub expected_guest_boot_id: String,
    /// Docker-root identity from the worker data recorded by the controller.
    pub docker_root: CleanupDockerRootIdentity,
    /// Docker daemon process identity.
    pub daemon: ProcessIdentity,
    /// Runner container process identity.
    pub runner: ProcessIdentity,
    /// `DinD` container process identity.
    pub dind: ProcessIdentity,
    /// Worker cgroup leaf paths recorded by the controller.
    pub cgroup_paths: [String; 2],
    /// Docker volume mountpoints from the selected daemon.
    pub volume_mountpoints: Vec<CleanupVolume>,
    /// Device identities for the eight tmpfs filesystems used by these workers.
    pub owned_tmpfs_devices: [[u32; 2]; 8],
}

/// Input for one cleanup inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupObserverRequest {
    /// Wire schema version.
    pub schema_version: u8,
    /// 32-character lower-case hexadecimal operation identity.
    pub operation_id: String,
    /// Digest of the binding data.
    pub binding_sha256: String,
    /// Identity data for the cleanup operation.
    pub binding: CleanupObserverBinding,
}

/// One Docker mountpoint and its volume role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupVolume {
    /// Volume role name from the controller contract.
    pub role: String,
    /// Mountpoint from the selected Docker daemon.
    pub mountpoint: String,
    /// Mount device measured before the worker stopped.
    pub mount_device: [u32; 2],
    /// `statx` device measured before the worker stopped.
    pub statx_device: [u32; 2],
    /// Directory inode measured before the worker stopped.
    pub inode: u64,
}

/// Process identity from Docker and journal records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessIdentity {
    /// Process ID in the guest process namespace.
    pub pid: u32,
    /// Process start time in clock ticks.
    pub start_ticks: u64,
    /// Mount namespace device values.
    pub namespace_device: [u32; 2],
    /// Mount namespace inode number.
    pub namespace_inode: u64,
}

/// Docker-root identity measured in a guest process namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupDockerRootIdentity {
    /// Specified mount target.
    pub target: String,
    /// Mount ID from mount information.
    pub mount_id: u64,
    /// Mount ID from `statx`.
    pub statx_mount_id: u64,
    /// Mount root from mount information.
    pub mount_root: String,
    /// Device values from mount information.
    pub mount_device: [u32; 2],
    /// Device values from `statx`.
    pub statx_device: [u32; 2],
    /// Filesystem type value from `statfs`.
    pub filesystem_type_magic: u64,
    /// Filesystem ID from `statvfs`.
    pub filesystem_id: u64,
    /// Read-only state from mount information.
    pub read_only: bool,
}

/// Data from one read-only cleanup inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupObserverRecord {
    /// Wire schema version.
    pub schema_version: u8,
    /// Operation identity from the request.
    pub operation_id: String,
    /// Digest of the binding data.
    pub binding_sha256: String,
    /// Guest boot identity measured by this process.
    pub guest_boot_id: Option<String>,
    /// Selected Docker-root identity measured before and after the inventory.
    pub docker_root: Option<CleanupDockerRootIdentity>,
    /// True when the program read all inventory data within the specified limits.
    pub complete: bool,
    /// True only when all owned references were absent.
    pub owned_references_absent: bool,
    /// Stable issue codes for incomplete inventory.
    pub issues: Vec<String>,
    /// Number of process tasks inspected.
    pub task_count: u32,
    /// Number of process descriptors inspected.
    pub descriptor_count: u32,
    /// Number of mount namespaces inspected.
    pub namespace_count: u32,
    /// Number of owned references found.
    pub owned_reference_count: u32,
    /// Volume mountpoints with no matching daemon mount.
    pub absent_volume_mountpoints: Vec<String>,
    /// Owned tmpfs devices with no matching mount or file descriptor.
    pub absent_tmpfs_devices: Vec<[u32; 2]>,
    /// Worker mount namespaces with no task or descriptor reference.
    pub absent_mount_namespaces: Vec<NamespaceIdentity>,
    /// Worker cgroup paths measured absent before and after the inventory.
    pub absent_cgroup_paths: Option<[String; 2]>,
}

/// Mount namespace identity measured from nsfs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamespaceIdentity {
    /// Namespace device values.
    pub device: [u32; 2],
    /// Namespace inode number.
    pub inode: u64,
}
