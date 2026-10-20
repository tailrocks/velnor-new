//! Records shared by the macOS controller and the Linux guest observer.

use serde::{Deserialize, Serialize};

mod request_validation;

pub use request_validation::validate_request;

/// Identity data from the controller, selected daemon, and worker containers for one cleanup inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupObserverBinding {
    /// Guest boot identity recorded by the controller.
    pub expected_guest_boot_id: String,
    /// The controller records the inode for the `JobFacts` directory after the initializer creates it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata_job_facts_inode: Option<u64>,
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

/// Validate the shared cleanup identity independently of the Linux observer.
///
/// This is also used when the macOS journal reads a persisted binding, so an
/// old or malformed record cannot become a cleanup authority.
///
/// # Errors
///
/// Returns an error if a required field is incorrect.
pub fn validate_binding(binding: &CleanupObserverBinding) -> Result<(), &'static str> {
    validate_boot_id(&binding.expected_guest_boot_id)?;
    validate_process(&binding.daemon)?;
    validate_process(&binding.runner)?;
    validate_process(&binding.dind)?;
    let root = &binding.docker_root;
    if root.target != "/velnor/docker-root"
        || root.mount_id == 0
        || root.mount_id != root.statx_mount_id
        || root.mount_device != root.statx_device
        || root.filesystem_id == 0
        || !root.read_only
    {
        return Err("cleanup_docker_root_identity");
    }
    if binding.daemon.pid == binding.runner.pid
        || binding.daemon.pid == binding.dind.pid
        || binding.runner.pid == binding.dind.pid
        || binding.runner.namespace_inode == binding.dind.namespace_inode
        || binding.cgroup_paths[0] == binding.cgroup_paths[1]
        || !matches!(binding.volume_mountpoints.len(), 4 | 5)
    {
        return Err("cleanup_binding_shape");
    }
    let expected_roles: &[&str] = if binding.volume_mountpoints.len() == 4 {
        &["shared-run", "runner-home", "runner-work", "dind-data"]
    } else {
        &[
            "shared-run",
            "runner-home",
            "runner-work",
            "dind-data",
            "engine-metadata",
        ]
    };
    for (volume, expected_role) in binding.volume_mountpoints.iter().zip(expected_roles) {
        if volume.role != *expected_role {
            return Err("cleanup_volume_role");
        }
        validate_absolute_path(&volume.mountpoint, "cleanup_volume_mountpoint")?;
        if volume.mount_device == [0, 0] || volume.statx_device == [0, 0] || volume.inode == 0 {
            return Err("cleanup_volume_identity");
        }
    }
    if binding.owned_tmpfs_devices.contains(&[0, 0]) {
        return Err("cleanup_tmpfs_device");
    }
    if binding.metadata_job_facts_inode == Some(0) {
        return Err("cleanup_job_facts_inode");
    }
    let mut devices = binding.owned_tmpfs_devices.to_vec();
    devices.sort_unstable();
    devices.dedup();
    if devices.len() != 8 {
        return Err("cleanup_tmpfs_duplicate");
    }
    for path in &binding.cgroup_paths {
        validate_absolute_path(path, "cleanup_cgroup_path")?;
    }
    Ok(())
}

fn validate_process(process: &ProcessIdentity) -> Result<(), &'static str> {
    if process.pid == 0
        || process.start_ticks == 0
        || process.namespace_inode == 0
        || process.namespace_device == [0, 0]
    {
        return Err("cleanup_process_identity");
    }
    Ok(())
}

fn validate_boot_id(value: &str) -> Result<(), &'static str> {
    let valid = value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        });
    valid.then_some(()).ok_or("cleanup_request_identity")
}

fn validate_absolute_path(value: &str, error: &'static str) -> Result<(), &'static str> {
    if value.len() > 4096
        || value == "/"
        || !value.starts_with('/')
        || value.contains("//")
        || value.split('/').skip(1).any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.bytes().any(|byte| {
                    !(byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
                })
        })
    {
        return Err(error);
    }
    Ok(())
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

/// Compare Docker root records from two mount namespaces.
///
/// Check that each record's mount ID matches its statx mount ID.
/// Check that each record's mount device pair matches its statx device pair.
/// Then compare the target, mount root, file system type, file system ID, mount device pair, and read-only state.
#[must_use]
pub fn docker_root_matches(
    expected: &CleanupDockerRootIdentity,
    observed: &CleanupDockerRootIdentity,
) -> bool {
    valid_local_docker_root(expected)
        && valid_local_docker_root(observed)
        && expected.target == observed.target
        && expected.mount_root == observed.mount_root
        && expected.mount_device == observed.mount_device
        && expected.filesystem_type_magic == observed.filesystem_type_magic
        && expected.filesystem_id == observed.filesystem_id
        && expected.read_only == observed.read_only
}

fn valid_local_docker_root(root: &CleanupDockerRootIdentity) -> bool {
    root.mount_id != 0
        && root.mount_id == root.statx_mount_id
        && root.mount_device == root.statx_device
        && root.filesystem_id != 0
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

#[cfg(test)]
#[path = "cleanup_observer_wire/tests.rs"]
mod tests;
