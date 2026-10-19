use serde::Serialize;

/// One cgroup ancestor's effective CPU set and quota.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CpuAncestorLimit {
    /// Exact path, ordered from cgroup-v2 root toward the probe leaf's parent.
    pub path: String,
    /// CPU count in this cgroup's effective cpuset.
    pub cpuset_cpus_count: u32,
    /// Ancestor CPU quota in microseconds, or `None` when unlimited.
    pub cpu_quota_micros: Option<u64>,
    /// Ancestor quota period in microseconds.
    pub cpu_period_micros: u64,
}

/// Version-5 bounded output record. `None` serializes as the required JSON `null` value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProbeRecord {
    /// Protocol schema version.
    pub schema_version: u8,
    /// Fixed path that the probe measured.
    pub docker_root_mount_target: String,
    /// Mount source from mount information.
    pub docker_root_mount_source: String,
    /// Mount root from mount information.
    pub docker_root_mount_root: String,
    /// Mount ID in the probe's mount namespace.
    pub docker_root_mount_id: u64,
    /// Mount device major number.
    pub docker_root_device_major: u32,
    /// Mount device minor number.
    pub docker_root_device_minor: u32,
    /// Device major number from statx on the retained Docker-root descriptor.
    pub docker_root_statx_device_major: u32,
    /// Device minor number from statx on the retained Docker-root descriptor.
    pub docker_root_statx_device_minor: u32,
    /// Filesystem type magic from `fstatfs`.
    pub docker_root_fs_type_magic: u64,
    /// Filesystem type name from mount information.
    pub docker_root_fs_type_name: String,
    /// Filesystem ID from `statvfs`.
    pub docker_root_filesystem_id: u64,
    /// Linux boot UUID for this guest.
    pub guest_boot_id: String,
    /// Guest Docker-root free bytes from checked `f_bfree * f_frsize`.
    pub docker_root_free_bytes: u64,
    /// Guest Docker-root total bytes from checked `f_blocks * f_frsize`.
    pub docker_root_total_bytes: u64,
    /// Guest Docker-root unprivileged available bytes.
    pub docker_root_available_bytes: u64,
    /// Raw filesystem inode total from `f_files`.
    pub docker_root_inode_total: u64,
    /// Raw filesystem free inodes from `f_ffree`.
    pub docker_root_inode_free: u64,
    /// Raw filesystem unprivileged available inodes from `f_favail`.
    pub docker_root_inode_available: u64,
    /// Inode policy derived from the reported type and raw counts.
    pub docker_root_inode_policy: crate::filesystem::InodePolicy,
    /// Guest `MemAvailable`, converted from KiB to bytes.
    pub memory_available_bytes: u64,
    /// Guest one-minute load rounded upward to thousandths.
    pub load_milli: u64,
    /// Optional PSI `some avg10` in hundredths of a percent.
    pub memory_psi_some_avg10_bps: Option<u64>,
    /// This path and its limits describe the probe process cgroup only.
    pub probe_cgroup_path: String,
    /// Maximum memory bytes for the probe's cgroup, or null for `max`.
    pub probe_cgroup_memory_max_bytes: Option<u64>,
    /// Current memory bytes charged to the probe's cgroup.
    pub probe_cgroup_memory_current_bytes: u64,
    /// Maximum swap bytes for the probe's cgroup, or null for `max`.
    pub probe_cgroup_swap_max_bytes: Option<u64>,
    /// Current swap bytes charged to the probe's cgroup.
    pub probe_cgroup_swap_current_bytes: u64,
    /// CPU quota in microseconds, or null for `max`.
    pub probe_cgroup_cpu_quota_micros: Option<u64>,
    /// CPU quota period in microseconds.
    pub probe_cgroup_cpu_period_micros: u64,
    /// Probe cgroup's effective CPU set count; its own quota is operational overhead.
    pub probe_cgroup_cpuset_cpus_count: u32,
    /// CPU limits above the probe leaf, in root-to-parent order.
    pub cpu_ancestor_limits: Vec<CpuAncestorLimit>,
    /// Host-visible PID from `/proc/self/status` `NSpid`.
    pub probe_host_pid: u32,
    /// PID from the probe's current PID namespace.
    pub probe_namespace_pid: u32,
    /// Maximum process count for the probe's cgroup, or null for `max`.
    pub probe_cgroup_pids_max: Option<u64>,
}

/// Maximum output record size, including its terminating newline.
pub const MAX_OUTPUT_BYTES: usize = 2048;
