// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use super::{AttachmentRole, WorkerRole};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageAttachmentObservation {
    pub role: AttachmentRole,

    pub target: String,

    pub read_only: bool,

    pub executable_mount: bool,

    pub mount_id: u64,

    pub statx_mount_id: u64,

    pub statx_inode: u64,

    pub mount_root: String,

    pub mount_source: String,

    pub mount_device_major: u32,

    pub mount_device_minor: u32,

    pub statx_device_major: u32,

    pub statx_device_minor: u32,

    pub filesystem_type_magic: u64,

    pub filesystem_type_name: String,

    pub filesystem_id: u64,

    pub block_size: u64,

    pub total_bytes: u64,

    pub free_bytes: u64,

    pub available_bytes: u64,

    pub inode_total: u64,

    pub inode_free: u64,

    pub inode_available: u64,

    pub inode_policy: crate::filesystem::InodePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelfLeafLimits {
    pub cgroup_path: String,

    pub memory_max_bytes: Option<u64>,

    pub memory_current_bytes: u64,

    pub swap_max_bytes: Option<u64>,

    pub swap_current_bytes: u64,

    pub cpu_quota_micros: Option<u64>,

    pub cpu_period_micros: u64,

    pub cpuset_cpus: Vec<u32>,

    pub pids_max: Option<u64>,

    pub pids_current: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelfStorageObservation {
    pub wire_mode: String,

    pub schema_version: u8,

    pub operation_id: String,

    pub container_id: String,

    pub role: WorkerRole,

    pub helper_protocol_version: u16,

    pub helper_source_revision: String,

    pub guest_boot_id: String,

    pub process_pid: u32,

    pub process_start_ticks: u64,

    pub init_start_ticks: u64,

    pub mount_namespace_id: u64,

    pub mount_namespace_device_major: u32,

    pub mount_namespace_device_minor: u32,

    pub cgroup_namespace_id: u64,

    pub cgroup_path_in_self_namespace: String,

    pub self_leaf_limits: SelfLeafLimits,

    pub attachments: Vec<StorageAttachmentObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessWitness {
    pub host_pid: u32,

    pub start_ticks: u64,

    pub mount_namespace_id: u64,

    pub mount_namespace_device_major: u32,

    pub mount_namespace_device_minor: u32,

    pub namespace_pids: Vec<u32>,

    pub cgroup_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootBackingKind {
    Upper,

    Work,

    Lower,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootBackingDirectoryObservation {
    pub kind: RootBackingKind,

    pub path: String,

    pub mount_root: String,

    pub mount_source: String,

    pub mount_filesystem_type: String,

    pub mount_read_only: bool,

    pub mount_device_major: u32,

    pub mount_device_minor: u32,

    pub statx_mount_id: u64,

    pub statx_device_major: u32,

    pub statx_device_minor: u32,

    pub filesystem_type_magic: u64,

    pub filesystem_id: u64,

    pub block_size: u64,

    pub total_bytes: u64,

    pub free_bytes: u64,

    pub available_bytes: u64,

    pub inode_total: u64,

    pub inode_free: u64,

    pub inode_available: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootBackingObservation {
    pub role: WorkerRole,

    pub container_id: String,

    pub guest_boot_id: String,

    pub init: ProcessWitness,

    pub helper: ProcessWitness,

    pub mount_source: String,

    pub filesystem_type: String,

    pub mount_id: u64,

    pub mount_device_major: u32,

    pub mount_device_minor: u32,

    pub mount_root: String,

    pub read_only: bool,

    pub upperdir: String,

    pub workdir: String,

    pub lowerdirs: Vec<String>,

    pub directories: Vec<RootBackingDirectoryObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CgroupNodeObservation {
    pub path: String,

    pub root: bool,

    pub memory_max_bytes: Option<u64>,

    pub memory_current_bytes: Option<u64>,

    pub swap_max_bytes: Option<u64>,

    pub swap_current_bytes: Option<u64>,

    pub cpu_quota_micros: Option<u64>,

    pub cpu_period_micros: Option<u64>,

    pub cpuset_cpus: Option<Vec<u32>>,

    pub pids_max: Option<u64>,

    pub pids_current: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedContainerCgroupObservation {
    pub role: WorkerRole,

    pub container_id: String,

    pub init: ProcessWitness,

    pub helper: ProcessWitness,

    pub hierarchy: Vec<CgroupNodeObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedCgroupObservation {
    pub wire_mode: String,

    pub schema_version: u8,

    pub operation_id: String,

    pub guest_boot_id: String,

    pub engine_metadata: StorageAttachmentObservation,

    pub docker_root_object: DockerRootObjectIdentity,

    pub daemon: ProcessWitness,

    pub containers: Vec<OwnedContainerCgroupObservation>,

    pub root_backings: Vec<RootBackingObservation>,
}

impl OwnedCgroupObservation {
    pub const SCHEMA_VERSION: u8 = 3;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DockerRootObjectIdentity {
    pub mount_id: u64,

    pub device_major: u32,

    pub device_minor: u32,

    pub inode: u64,

    pub filesystem_type_magic: u64,

    pub filesystem_id: u64,
}
