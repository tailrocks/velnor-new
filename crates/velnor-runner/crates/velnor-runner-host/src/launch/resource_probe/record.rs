//! Strict schema-3 output parsing for the isolated guest probe.

use serde::{Deserialize, Serialize};

pub(super) const MAX_OUTPUT_BYTES: usize = 2048;
const SCHEMA_VERSION: u8 = 3;
const MAX_PSI_BPS: u64 = 10_000;
const DOCKER_ROOT_TARGET: &str = "/velnor/docker-root";
const BTRFS_MAGIC: u64 = 0x9123_683e;
const TMPFS_MAGIC: u64 = 0x0102_1994;

/// Probe-reported filesystem inode policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum InodePolicy {
    /// The filesystem reports finite inode counts.
    Finite,
    /// Btrfs allocates inodes dynamically and reports zero counts.
    DynamicBtrfs,
    /// The probe cannot prove an inode limit.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CpuAncestorLimit {
    pub(super) path: String,
    pub(super) cpuset_cpus_count: u32,
    pub(super) cpu_quota_micros: Option<u64>,
    pub(super) cpu_period_micros: u64,
}

/// One validated output record. It stays in memory for the current attempt.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProbeRecord {
    /// Producer protocol version.
    pub(super) schema_version: u8,
    /// Fixed path that the probe measured.
    pub(super) docker_root_mount_target: String,
    /// Mount ID in the probe's mount namespace. It is not a stable pool ID.
    pub(super) docker_root_mount_id: u64,
    /// Mount device major number.
    pub(super) docker_root_device_major: u32,
    /// Mount device minor number.
    pub(super) docker_root_device_minor: u32,
    /// Device major number from `statx` on the retained Docker-root descriptor.
    pub(super) docker_root_statx_device_major: u32,
    /// Device minor number from `statx` on the retained Docker-root descriptor.
    pub(super) docker_root_statx_device_minor: u32,
    /// Filesystem type magic from `fstatfs`.
    pub(super) docker_root_fs_type_magic: u64,
    /// Filesystem type name from mount information.
    pub(super) docker_root_fs_type_name: String,
    /// Filesystem ID from `statvfs`.
    pub(super) docker_root_filesystem_id: u64,
    /// Linux boot UUID for this guest.
    pub(super) guest_boot_id: String,
    /// Guest Docker-root free bytes from checked `f_bfree * f_frsize`.
    pub(super) docker_root_free_bytes: u64,
    /// Guest Docker-root total bytes from checked `f_blocks * f_frsize`.
    pub(super) docker_root_total_bytes: u64,
    /// Guest Docker-root unprivileged available bytes.
    pub(super) docker_root_available_bytes: u64,
    /// Raw filesystem inode total from `f_files`.
    pub(super) docker_root_inode_total: u64,
    /// Raw filesystem free inodes from `f_ffree`.
    pub(super) docker_root_inode_free: u64,
    /// Raw filesystem unprivileged available inodes from `f_favail`.
    pub(super) docker_root_inode_available: u64,
    /// Producer-classified inode policy.
    pub(super) docker_root_inode_policy: InodePolicy,
    /// Guest memory available to new work.
    pub(super) memory_available_bytes: u64,
    /// Guest one-minute load rounded upward to thousandths.
    pub(super) load_milli: u64,
    /// Optional diagnostic PSI value in basis points.
    pub(super) memory_psi_some_avg10_bps: Option<u64>,
    /// Probe process cgroup path, not the worker cgroup path.
    pub(super) probe_cgroup_path: String,
    /// Probe cgroup memory limit, or null when unlimited.
    pub(super) probe_cgroup_memory_max_bytes: Option<u64>,
    /// Current memory charged to the probe cgroup.
    pub(super) probe_cgroup_memory_current_bytes: u64,
    /// Probe cgroup swap limit, or null when unlimited.
    pub(super) probe_cgroup_swap_max_bytes: Option<u64>,
    /// Current swap charged to the probe cgroup.
    pub(super) probe_cgroup_swap_current_bytes: u64,
    /// Probe cgroup CPU quota, or null when unlimited.
    pub(super) probe_cgroup_cpu_quota_micros: Option<u64>,
    /// Probe cgroup CPU quota period.
    pub(super) probe_cgroup_cpu_period_micros: u64,
    /// Probe cgroup's effective CPU set count; its own quota is operational overhead.
    pub(super) probe_cgroup_cpuset_cpus_count: u32,
    /// CPU limits above the probe leaf, in root-to-parent order.
    pub(super) cpu_ancestor_limits: Vec<CpuAncestorLimit>,
    /// Host-visible PID from `/proc/self/status` `NSpid`.
    pub(super) probe_host_pid: u32,
    /// PID from the probe's current PID namespace.
    pub(super) probe_namespace_pid: u32,
    /// Probe cgroup process limit, or null when unlimited.
    pub(super) probe_cgroup_pids_max: Option<u64>,
}

impl ProbeRecord {
    /// Parse canonical one-line JSON and bind its memory value to daemon info.
    pub(super) fn parse(output: &[u8], memory_total_bytes: u64) -> Option<Self> {
        if !valid_framing(output) || memory_total_bytes == 0 {
            return None;
        }
        let body = output.get(..output.len().checked_sub(1)?)?;
        let mut deserializer = serde_json::Deserializer::from_slice(body);
        let record = Self::deserialize(&mut deserializer).ok()?;
        deserializer.end().ok()?;
        (record.valid_values(memory_total_bytes) && canonical_body_matches(&record, body))
            .then_some(record)
    }

    fn valid_values(&self, memory_total_bytes: u64) -> bool {
        self.schema_version == SCHEMA_VERSION
            && self.docker_root_mount_target == DOCKER_ROOT_TARGET
            && self.docker_root_mount_id > 0
            && !self.docker_root_fs_type_name.is_empty()
            && self.filesystem_identity_valid()
            && valid_boot_id(&self.guest_boot_id)
            && self.docker_root_total_bytes > 0
            && self.docker_root_free_bytes <= self.docker_root_total_bytes
            && self.docker_root_available_bytes <= self.docker_root_free_bytes
            && self.docker_root_memory_values_valid(memory_total_bytes)
            && self.inode_values_valid()
            && self.cgroup_values_valid()
            && self
                .effective_cpu_millicores(u32::MAX)
                .is_some_and(|value| value > 0)
            && self
                .memory_psi_some_avg10_bps
                .is_none_or(|value| value <= MAX_PSI_BPS)
    }

    fn docker_root_memory_values_valid(&self, memory_total_bytes: u64) -> bool {
        self.memory_available_bytes <= memory_total_bytes
    }

    fn filesystem_identity_valid(&self) -> bool {
        let btrfs = self.docker_root_fs_type_magic == BTRFS_MAGIC;
        let type_matches_magic = match self.docker_root_fs_type_name.as_str() {
            "btrfs" => btrfs,
            "tmpfs" => self.docker_root_fs_type_magic == TMPFS_MAGIC,
            _ => !matches!(self.docker_root_fs_type_magic, BTRFS_MAGIC | TMPFS_MAGIC),
        };
        type_matches_magic
            && (btrfs
                || (self.docker_root_device_major == self.docker_root_statx_device_major
                    && self.docker_root_device_minor == self.docker_root_statx_device_minor))
    }

    fn inode_values_valid(&self) -> bool {
        if self.docker_root_inode_available > self.docker_root_inode_free
            || self.docker_root_inode_free > self.docker_root_inode_total
        {
            return false;
        }
        match self.docker_root_inode_policy {
            InodePolicy::Finite => {
                self.docker_root_fs_type_magic != BTRFS_MAGIC && self.docker_root_inode_total > 0
            }
            InodePolicy::DynamicBtrfs => {
                self.docker_root_fs_type_magic == BTRFS_MAGIC
                    && self.docker_root_fs_type_name == "btrfs"
                    && self.docker_root_inode_total == 0
                    && self.docker_root_inode_free == 0
                    && self.docker_root_inode_available == 0
            }
            InodePolicy::Unknown => true,
        }
    }

    fn cgroup_values_valid(&self) -> bool {
        let path = self.probe_cgroup_path.as_str();
        if path.is_empty()
            || path == "/"
            || path.len() > 256
            || !path.starts_with('/')
            || path.contains("//")
            || path.chars().any(char::is_control)
            || self.probe_cgroup_cpu_period_micros == 0
            || self.probe_cgroup_cpuset_cpus_count == 0
            || self.probe_cgroup_cpuset_cpus_count > 4096
            || self.probe_host_pid == 0
            || self.probe_namespace_pid == 0
            || self.probe_host_pid != self.probe_namespace_pid
        {
            return false;
        }
        bounded_by(
            self.probe_cgroup_memory_current_bytes,
            self.probe_cgroup_memory_max_bytes,
        ) && bounded_by(
            self.probe_cgroup_swap_current_bytes,
            self.probe_cgroup_swap_max_bytes,
        ) && self
            .probe_cgroup_cpu_quota_micros
            .is_none_or(|quota| quota > 0)
            && self.probe_cgroup_pids_max.is_none_or(|limit| limit > 0)
            && self.cpu_ancestry_valid()
    }

    fn cpu_ancestry_valid(&self) -> bool {
        let Some(paths) = expected_ancestor_paths(&self.probe_cgroup_path) else {
            return false;
        };
        if paths.len() != self.cpu_ancestor_limits.len() {
            return false;
        }
        let mut parent_cpuset = None;
        for (expected_path, ancestor) in paths.iter().zip(&self.cpu_ancestor_limits) {
            if ancestor.path != *expected_path
                || ancestor.cpuset_cpus_count == 0
                || ancestor.cpuset_cpus_count > 4096
                || ancestor.cpu_period_micros == 0
                || ancestor.cpu_quota_micros.is_some_and(|quota| quota == 0)
                || parent_cpuset.is_some_and(|count| ancestor.cpuset_cpus_count > count)
            {
                return false;
            }
            parent_cpuset = Some(ancestor.cpuset_cpus_count);
        }
        parent_cpuset.is_none_or(|count| self.probe_cgroup_cpuset_cpus_count <= count)
    }

    pub(super) fn effective_cpu_millicores(&self, ncpu: u32) -> Option<u64> {
        if ncpu == 0 || self.probe_cgroup_cpuset_cpus_count == 0 {
            return None;
        }
        let (expected_period, expected_quota, _, _, _) = super::projection::resource_limits();
        if self.probe_cgroup_cpu_period_micros != u64::try_from(expected_period).ok()?
            || self.probe_cgroup_cpu_quota_micros != Some(u64::try_from(expected_quota).ok()?)
        {
            return None;
        }
        let mut effective = u64::from(self.probe_cgroup_cpuset_cpus_count).checked_mul(1_000)?;
        for ancestor in &self.cpu_ancestor_limits {
            let cpuset = u64::from(ancestor.cpuset_cpus_count).checked_mul(1_000)?;
            effective = effective.min(cpuset);
            if let Some(quota) = ancestor.cpu_quota_micros {
                let quota_millicores = u128::from(quota)
                    .checked_mul(1_000)?
                    .checked_div(u128::from(ancestor.cpu_period_micros))?;
                effective = effective.min(u64::try_from(quota_millicores).ok()?);
            }
        }
        Some(effective.min(u64::from(ncpu).checked_mul(1_000)?))
    }
}

fn expected_ancestor_paths(path: &str) -> Option<Vec<String>> {
    if !path.starts_with('/') || path.contains("//") || path.len() > 256 {
        return None;
    }
    let components = if path == "/" {
        Vec::new()
    } else {
        path.split('/').skip(1).collect::<Vec<_>>()
    };
    if components.len() > 16
        || components.iter().any(|part| {
            part.is_empty()
                || *part == "."
                || *part == ".."
                || part.contains('\\')
                || part.contains(':')
                || part.chars().any(char::is_control)
        })
    {
        return None;
    }
    let mut paths = Vec::with_capacity(components.len());
    paths.push("/".to_owned());
    let mut current = String::from("/");
    for component in components.iter().take(components.len().saturating_sub(1)) {
        if current != "/" {
            current.push('/');
        }
        current.push_str(component);
        paths.push(current.clone());
    }
    Some(paths)
}

fn bounded_by(current: u64, limit: Option<u64>) -> bool {
    limit.is_none_or(|limit| current <= limit)
}

fn valid_boot_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()
            }
        })
}

fn canonical_body_matches(record: &ProbeRecord, body: &[u8]) -> bool {
    serde_json::to_vec(record).is_ok_and(|canonical| canonical == body)
}

fn valid_framing(output: &[u8]) -> bool {
    output.len() >= 2
        && output.len() <= MAX_OUTPUT_BYTES
        && output.last() == Some(&b'\n')
        && output[..output.len() - 1].iter().all(|byte| *byte != b'\n')
}

#[cfg(test)]
pub(super) fn fixture_record() -> ProbeRecord {
    ProbeRecord {
        schema_version: SCHEMA_VERSION,
        docker_root_mount_target: DOCKER_ROOT_TARGET.to_owned(),
        docker_root_mount_id: 5,
        docker_root_device_major: 0,
        docker_root_device_minor: 38,
        docker_root_statx_device_major: 0,
        docker_root_statx_device_minor: 41,
        docker_root_fs_type_magic: 0x9123_683e,
        docker_root_fs_type_name: "btrfs".to_owned(),
        docker_root_filesystem_id: 17,
        guest_boot_id: "12345678-1234-1234-1234-123456789abc".to_owned(),
        docker_root_free_bytes: 20 * 1024 * 1024 * 1024,
        docker_root_total_bytes: 40 * 1024 * 1024 * 1024,
        docker_root_available_bytes: 19 * 1024 * 1024 * 1024,
        docker_root_inode_total: 0,
        docker_root_inode_free: 0,
        docker_root_inode_available: 0,
        docker_root_inode_policy: InodePolicy::DynamicBtrfs,
        memory_available_bytes: 8 * 1024 * 1024 * 1024,
        load_milli: 125,
        memory_psi_some_avg10_bps: None,
        probe_cgroup_path: "/system.slice/velnor-resource-probe.service".to_owned(),
        probe_cgroup_memory_max_bytes: Some(512 * 1024 * 1024),
        probe_cgroup_memory_current_bytes: 64 * 1024 * 1024,
        probe_cgroup_swap_max_bytes: Some(0),
        probe_cgroup_swap_current_bytes: 0,
        probe_cgroup_cpu_quota_micros: Some(10_000),
        probe_cgroup_cpu_period_micros: 100_000,
        probe_cgroup_cpuset_cpus_count: 8,
        cpu_ancestor_limits: vec![
            CpuAncestorLimit {
                path: "/".to_owned(),
                cpuset_cpus_count: 8,
                cpu_quota_micros: None,
                cpu_period_micros: 100_000,
            },
            CpuAncestorLimit {
                path: "/system.slice".to_owned(),
                cpuset_cpus_count: 8,
                cpu_quota_micros: None,
                cpu_period_micros: 100_000,
            },
        ],
        probe_host_pid: 1234,
        probe_namespace_pid: 1234,
        probe_cgroup_pids_max: Some(64),
    }
}

#[cfg(test)]
#[path = "record/schema3_tests.rs"]
mod schema3_tests;
