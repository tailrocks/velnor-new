// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CpuAncestorLimit {
    pub path: String,

    pub cpuset_cpus_count: u32,

    pub cpu_quota_micros: Option<u64>,

    pub cpu_period_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProbeRecord {
    pub schema_version: u8,

    pub docker_root_mount_target: String,

    pub docker_root_mount_source: String,

    pub docker_root_mount_root: String,

    pub docker_root_mount_id: u64,

    pub docker_root_device_major: u32,

    pub docker_root_device_minor: u32,

    pub docker_root_statx_device_major: u32,

    pub docker_root_statx_device_minor: u32,

    pub docker_root_fs_type_magic: u64,

    pub docker_root_fs_type_name: String,

    pub docker_root_filesystem_id: u64,

    pub guest_boot_id: String,

    pub docker_root_free_bytes: u64,

    pub docker_root_total_bytes: u64,

    pub docker_root_available_bytes: u64,

    pub docker_root_inode_total: u64,

    pub docker_root_inode_free: u64,

    pub docker_root_inode_available: u64,

    pub docker_root_inode_policy: crate::filesystem::InodePolicy,

    pub memory_available_bytes: u64,

    pub load_milli: u64,

    pub memory_psi_some_avg10_bps: Option<u64>,

    pub probe_cgroup_path: String,

    pub probe_cgroup_memory_max_bytes: Option<u64>,

    pub probe_cgroup_memory_current_bytes: u64,

    pub probe_cgroup_swap_max_bytes: Option<u64>,

    pub probe_cgroup_swap_current_bytes: u64,

    pub probe_cgroup_cpu_quota_micros: Option<u64>,

    pub probe_cgroup_cpu_period_micros: u64,

    pub probe_cgroup_cpuset_cpus_count: u32,

    pub cpu_ancestor_limits: Vec<CpuAncestorLimit>,

    pub probe_host_pid: u32,

    pub probe_namespace_pid: u32,

    pub probe_cgroup_pids_max: Option<u64>,
}

pub const MAX_OUTPUT_BYTES: usize = 2048;
