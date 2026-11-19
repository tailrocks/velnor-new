// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::TestResult;
use velnor_resource_probe::{CpuAncestorLimit, InodePolicy, ProbeRecord};

pub(super) fn schema5_record_bytes(schema_version: u8) -> TestResult<Vec<u8>> {
    let record = ProbeRecord {
        schema_version,
        docker_root_mount_target: "/velnor/docker-root".to_owned(),
        docker_root_mount_source: "/dev/vda1".to_owned(),
        docker_root_mount_root: "/var/lib/docker".to_owned(),
        docker_root_mount_id: 42,
        docker_root_device_major: 254,
        docker_root_device_minor: 1,
        docker_root_statx_device_major: 254,
        docker_root_statx_device_minor: 1,
        docker_root_fs_type_magic: 61267,
        docker_root_fs_type_name: "ext4".to_owned(),
        docker_root_filesystem_id: 9001,
        guest_boot_id: "01234567-89ab-cdef-0123-456789abcdef".to_owned(),
        docker_root_free_bytes: 50_000_000_000,
        docker_root_total_bytes: 100_000_000_000,
        docker_root_available_bytes: 45_000_000_000,
        docker_root_inode_total: 1_000_000,
        docker_root_inode_free: 900_000,
        docker_root_inode_available: 900_000,
        docker_root_inode_policy: InodePolicy::Finite,
        memory_available_bytes: 8_589_934_592,
        load_milli: 250,
        memory_psi_some_avg10_bps: Some(0),
        probe_cgroup_path: "/docker/probe-1".to_owned(),
        probe_cgroup_memory_max_bytes: Some(67_108_864),
        probe_cgroup_memory_current_bytes: 4_194_304,
        probe_cgroup_swap_max_bytes: Some(0),
        probe_cgroup_swap_current_bytes: 0,
        probe_cgroup_cpu_quota_micros: Some(25_000),
        probe_cgroup_cpu_period_micros: 100_000,
        probe_cgroup_cpuset_cpus_count: 4,
        cpu_ancestor_limits: vec![
            CpuAncestorLimit {
                path: "/".to_owned(),
                cpuset_cpus_count: 4,
                cpu_quota_micros: None,
                cpu_period_micros: 100_000,
            },
            CpuAncestorLimit {
                path: "/docker".to_owned(),
                cpuset_cpus_count: 4,
                cpu_quota_micros: None,
                cpu_period_micros: 100_000,
            },
        ],
        probe_host_pid: 1234,
        probe_namespace_pid: 1234,
        probe_cgroup_pids_max: Some(64),
    };
    let mut bytes = serde_json::to_vec(&record)?;
    bytes.push(b'\n');
    Ok(bytes)
}
