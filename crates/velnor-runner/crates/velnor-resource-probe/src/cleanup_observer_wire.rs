// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

mod request_validation;

pub use request_validation::validate_request;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupObserverBinding {
    pub expected_guest_boot_id: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata_job_facts_inode: Option<u64>,

    pub docker_root: CleanupDockerRootIdentity,

    pub daemon: ProcessIdentity,

    pub runner: ProcessIdentity,

    pub dind: ProcessIdentity,

    pub cgroup_paths: [String; 2],

    pub volume_mountpoints: Vec<CleanupVolume>,

    pub owned_tmpfs_devices: [[u32; 2]; 8],
}

pub fn validate_binding(binding: &CleanupObserverBinding) -> Result<(), &'static str> {
    validate_boot_id(&binding.expected_guest_boot_id)?;
    validate_process(&binding.daemon)?;
    validate_process(&binding.runner)?;
    validate_process(&binding.dind)?;
    let root = &binding.docker_root;
    if root.target != "/velnor/docker-root"
        || root.mount_id == 0
        || root.mount_id != root.statx_mount_id
        || (root.filesystem_type_magic != 0x9123_683e && root.mount_device != root.statx_device)
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupObserverRequest {
    pub schema_version: u8,

    pub operation_id: String,

    pub binding_sha256: String,

    pub binding: CleanupObserverBinding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupVolume {
    pub role: String,

    pub mountpoint: String,

    pub mount_device: [u32; 2],

    pub statx_device: [u32; 2],

    pub inode: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessIdentity {
    pub pid: u32,

    pub start_ticks: u64,

    pub namespace_device: [u32; 2],

    pub namespace_inode: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupDockerRootIdentity {
    pub target: String,

    pub mount_id: u64,

    pub statx_mount_id: u64,

    pub mount_root: String,

    pub mount_device: [u32; 2],

    pub statx_device: [u32; 2],

    pub filesystem_type_magic: u64,

    pub filesystem_id: u64,

    pub read_only: bool,
}

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
        && expected.statx_device == observed.statx_device
        && expected.filesystem_type_magic == observed.filesystem_type_magic
        && expected.filesystem_id == observed.filesystem_id
        && expected.read_only == observed.read_only
}

fn valid_local_docker_root(root: &CleanupDockerRootIdentity) -> bool {
    root.mount_id != 0
        && root.mount_id == root.statx_mount_id
        && (root.filesystem_type_magic == 0x9123_683e || root.mount_device == root.statx_device)
        && root.filesystem_id != 0
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupObserverRecord {
    pub schema_version: u8,

    pub operation_id: String,

    pub binding_sha256: String,

    pub guest_boot_id: Option<String>,

    pub docker_root: Option<CleanupDockerRootIdentity>,

    pub complete: bool,

    pub owned_references_absent: bool,

    pub issues: Vec<String>,

    pub task_count: u32,

    pub descriptor_count: u32,

    pub namespace_count: u32,

    pub owned_reference_count: u32,

    pub absent_volume_mountpoints: Vec<String>,

    pub absent_tmpfs_devices: Vec<[u32; 2]>,

    pub absent_mount_namespaces: Vec<NamespaceIdentity>,

    pub absent_cgroup_paths: Option<[String; 2]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamespaceIdentity {
    pub device: [u32; 2],

    pub inode: u64,
}
