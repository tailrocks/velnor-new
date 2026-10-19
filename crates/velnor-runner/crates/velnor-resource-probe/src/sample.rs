use std::{fs::File, io::Read, path::PathBuf, process};

use crate::{
    ProbeError, ProbeRecord,
    cgroup::{ProbeCgroup, namespace_pids, observe_hierarchy, verify_process_membership},
    filesystem::{filesystem_name, observe_docker_root, parse_boot_id},
    load,
    memory::available_bytes,
    mountinfo::{attachment_mount_source, cgroup2_root},
    pressure::memory_some_avg10,
};

const MEMINFO_PATH: &str = "/proc/meminfo";
const LOADAVG_PATH: &str = "/proc/loadavg";
const PRESSURE_PATH: &str = "/proc/pressure/memory";
const MOUNTINFO_PATH: &str = "/proc/self/mountinfo";
const CGROUP_PATH: &str = "/proc/self/cgroup";
const STATUS_PATH: &str = "/proc/self/status";
const BOOT_ID_PATH: &str = "/proc/sys/kernel/random/boot_id";
const CGROUP_ROOT: &str = "/sys/fs/cgroup";
const MEMINFO_LIMIT: usize = 16 * 1024;
const LOADAVG_LIMIT: usize = 256;
const PRESSURE_LIMIT: usize = 4096;
const MOUNTINFO_LIMIT: usize = 64 * 1024;
const SMALL_LIMIT: usize = 4096;

/// Read fixed, bounded guest resource facts into one versioned record.
///
/// # Errors
///
/// Returns an error when a required fact is missing, malformed, or too large.
pub fn sample() -> Result<ProbeRecord, ProbeError> {
    let mountinfo = read_bounded(MOUNTINFO_PATH, MOUNTINFO_LIMIT, "mountinfo")?;
    let cgroup_mount = cgroup2_root(&mountinfo)?;
    let filesystem = observe_docker_root(&mountinfo)?;
    let docker_root_fs_type_name = filesystem_name(&filesystem.mount, filesystem.fs_type_magic)?;
    let guest_boot_id = parse_boot_id(&read_bounded(BOOT_ID_PATH, 128, "boot_id")?)?;
    let memory_available_bytes =
        available_bytes(&read_bounded(MEMINFO_PATH, MEMINFO_LIMIT, "meminfo")?)?;
    let load_milli =
        load::one_minute_milli(&read_bounded(LOADAVG_PATH, LOADAVG_LIMIT, "loadavg")?)?;
    let memory_psi_some_avg10_bps = read_bounded(PRESSURE_PATH, PRESSURE_LIMIT, "psi")
        .ok()
        .and_then(|bytes| memory_some_avg10(&bytes));
    let process_cgroup = observe_process_cgroup(&cgroup_mount)?;
    let cgroup = process_cgroup.facts;
    let cpu_hierarchy = process_cgroup.cpu;
    let pid_pair = process_cgroup.pids;
    Ok(ProbeRecord {
        schema_version: 5,
        docker_root_mount_target: filesystem.mount_target.to_owned(),
        docker_root_mount_source: attachment_mount_source(&mountinfo, filesystem.mount_target)?,
        docker_root_mount_root: filesystem.mount.root.clone(),
        docker_root_mount_id: filesystem.mount.mount_id,
        docker_root_device_major: filesystem.mount.device_major,
        docker_root_device_minor: filesystem.mount.device_minor,
        docker_root_statx_device_major: filesystem.statx_device_major,
        docker_root_statx_device_minor: filesystem.statx_device_minor,
        docker_root_fs_type_magic: filesystem.fs_type_magic,
        docker_root_fs_type_name,
        docker_root_filesystem_id: filesystem.filesystem_id,
        guest_boot_id,
        docker_root_free_bytes: filesystem.free_bytes,
        docker_root_total_bytes: filesystem.total_bytes,
        docker_root_available_bytes: filesystem.available_bytes,
        docker_root_inode_total: filesystem.total_inodes,
        docker_root_inode_free: filesystem.free_inodes,
        docker_root_inode_available: filesystem.available_inodes,
        docker_root_inode_policy: filesystem.inode_policy,
        memory_available_bytes,
        load_milli,
        memory_psi_some_avg10_bps,
        probe_cgroup_path: cgroup.path,
        probe_cgroup_memory_max_bytes: cgroup.memory_max_bytes,
        probe_cgroup_memory_current_bytes: cgroup.memory_current_bytes,
        probe_cgroup_swap_max_bytes: cgroup.swap_max_bytes,
        probe_cgroup_swap_current_bytes: cgroup.swap_current_bytes,
        probe_cgroup_cpu_quota_micros: cgroup.cpu_quota_micros,
        probe_cgroup_cpu_period_micros: cgroup.cpu_period_micros,
        probe_cgroup_cpuset_cpus_count: cpu_hierarchy.leaf_cpuset_cpus_count,
        cpu_ancestor_limits: cpu_hierarchy.ancestors,
        probe_host_pid: pid_pair.host_pid,
        probe_namespace_pid: pid_pair.namespace_pid,
        probe_cgroup_pids_max: cgroup.pids_max,
    })
}

struct ProcessCgroupObservation {
    facts: ProbeCgroup,
    cpu: crate::cgroup::CpuHierarchy,
    pids: crate::cgroup::NamespacePids,
}

fn observe_process_cgroup(
    cgroup_mount: &crate::mountinfo::MountEntry,
) -> Result<ProcessCgroupObservation, ProbeError> {
    let cgroup_source = read_bounded(CGROUP_PATH, SMALL_LIMIT, "proc_cgroup")?;
    let cgroup_path = probe_cgroup_path(&cgroup_source)?;
    let process_status = read_bounded(STATUS_PATH, SMALL_LIMIT, "proc_status")?;
    let process_id = process::id();
    let pids = namespace_pids(&process_status, process_id)?;
    verify_process_membership(&cgroup_path, pids.namespace_pid)?;
    let cpu = observe_hierarchy(&cgroup_path)?;
    let cgroup_dir = cgroup_directory(&cgroup_path)?;
    let facts = ProbeCgroup::parse(
        &cgroup_path,
        &read_cgroup_file(&cgroup_dir, "memory.max")?,
        &read_cgroup_file(&cgroup_dir, "memory.current")?,
        &read_cgroup_file(&cgroup_dir, "memory.swap.max")?,
        &read_cgroup_file(&cgroup_dir, "memory.swap.current")?,
        &read_cgroup_file(&cgroup_dir, "cpu.max")?,
        &read_cgroup_file(&cgroup_dir, "pids.max")?,
    )?;
    if cpu.leaf_quota_micros != facts.cpu_quota_micros
        || cpu.leaf_period_micros != facts.cpu_period_micros
    {
        return Err(ProbeError::Invalid("cgroup_cpu_changed"));
    }
    let path_after = probe_cgroup_path(&read_bounded(CGROUP_PATH, SMALL_LIMIT, "proc_cgroup")?)?;
    let mount_after = cgroup2_root(&read_bounded(MOUNTINFO_PATH, MOUNTINFO_LIMIT, "mountinfo")?)?;
    let status_after = read_bounded(STATUS_PATH, SMALL_LIMIT, "proc_status")?;
    let pids_after = namespace_pids(&status_after, process_id)?;
    verify_process_membership(&cgroup_path, pids.namespace_pid)?;
    let cpu_after = observe_hierarchy(&path_after)?;
    if path_after != cgroup_path
        || &mount_after != cgroup_mount
        || pids_after != pids
        || cpu_after != cpu
    {
        return Err(ProbeError::Invalid("cgroup_observation_changed"));
    }
    Ok(ProcessCgroupObservation { facts, cpu, pids })
}

fn probe_cgroup_path(input: &[u8]) -> Result<String, ProbeError> {
    let text = std::str::from_utf8(input).map_err(|_| ProbeError::Invalid("proc_cgroup_utf8"))?;
    let mut path = None;
    for line in text.lines() {
        let Some((hierarchy, rest)) = line.split_once(':') else {
            return Err(ProbeError::Invalid("proc_cgroup_format"));
        };
        let Some((controllers, value)) = rest.split_once(':') else {
            return Err(ProbeError::Invalid("proc_cgroup_format"));
        };
        if hierarchy == "0" && controllers.is_empty() && path.replace(value.to_owned()).is_some() {
            return Err(ProbeError::Invalid("proc_cgroup_duplicate"));
        }
    }
    let path = path.ok_or(ProbeError::Invalid("proc_cgroup_v2_missing"))?;
    if path.is_empty()
        || !path.starts_with('/')
        || path.split('/').any(|part| part == "..")
        || path.contains("//")
    {
        return Err(ProbeError::Invalid("proc_cgroup_path"));
    }
    Ok(path)
}

fn cgroup_directory(path: &str) -> Result<PathBuf, ProbeError> {
    let relative = path
        .strip_prefix('/')
        .ok_or(ProbeError::Invalid("proc_cgroup_path"))?;
    let mut directory = PathBuf::from(CGROUP_ROOT);
    if !relative.is_empty() {
        directory.push(relative);
    }
    Ok(directory)
}

fn read_cgroup_file(
    directory: &std::path::Path,
    name: &'static str,
) -> Result<Vec<u8>, ProbeError> {
    let path = directory.join(name);
    let file = File::open(path).map_err(|error| ProbeError::Read(name, error))?;
    let mut bytes = Vec::with_capacity(128);
    file.take(129)
        .read_to_end(&mut bytes)
        .map_err(|error| ProbeError::Read(name, error))?;
    if bytes.len() > 128 {
        return Err(ProbeError::InputTooLarge(name));
    }
    Ok(bytes)
}

fn read_bounded(path: &str, limit: usize, name: &'static str) -> Result<Vec<u8>, ProbeError> {
    let file = File::open(path).map_err(|error| ProbeError::Read(name, error))?;
    let mut bytes = Vec::with_capacity(limit.min(4096));
    file.take(u64::try_from(limit + 1).map_err(|_| ProbeError::Overflow("input_limit"))?)
        .read_to_end(&mut bytes)
        .map_err(|error| ProbeError::Read(name, error))?;
    if bytes.len() > limit {
        return Err(ProbeError::InputTooLarge(name));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_one_unified_cgroup_path() -> Result<(), ProbeError> {
        assert_eq!(probe_cgroup_path(b"0::/runner/job\n")?, "/runner/job");
        assert!(probe_cgroup_path(b"1:cpu:/x\n").is_err());
        assert!(probe_cgroup_path(b"0::/x\n0::/y\n").is_err());
        assert!(probe_cgroup_path(b"0::/x/../host\n").is_err());
        Ok(())
    }
}
