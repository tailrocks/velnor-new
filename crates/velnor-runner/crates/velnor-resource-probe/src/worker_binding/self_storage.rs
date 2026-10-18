use std::{fs, io::Read, path::PathBuf, process};

use crate::{
    ProbeError,
    cgroup::{ProbeCgroup, parse_cpuset, verify_process_membership},
    filesystem::parse_boot_id,
    worker_binding::{
        SelfLeafLimits, SelfStorageObservation, SelfStorageRequest, WORKER_PROTOCOL_VERSION,
    },
};

const MOUNTINFO_PATH: &str = "/proc/self/mountinfo";
const CGROUP_PATH: &str = "/proc/self/cgroup";
const BOOT_ID_PATH: &str = "/proc/sys/kernel/random/boot_id";
const CGROUP_ROOT: &str = "/sys/fs/cgroup";
const PROC_LIMIT: usize = 4096;
const MOUNTINFO_LIMIT: usize = 65_536;
const CGROUP_FILE_LIMIT: usize = 4096;

pub(super) fn observe(request: SelfStorageRequest) -> Result<SelfStorageObservation, ProbeError> {
    request.validate()?;
    let helper_source_revision = source_revision()?;
    let process_pid = process::id();
    let helper_start_ticks = process_start_ticks("self", process_pid)?;
    let init_start_ticks = process_start_ticks("1", 1)?;
    let mount_namespace_id = namespace_inode("mnt")?;
    let cgroup_namespace_id = namespace_inode("cgroup")?;
    let cgroup_path_in_self_namespace = self_cgroup_path()?;
    let self_leaf_limits = observe_leaf(&cgroup_path_in_self_namespace, process_pid)?;
    let mountinfo = read_bounded(MOUNTINFO_PATH, MOUNTINFO_LIMIT, "mountinfo")?;
    let attachments = super::storage::observe_attachments(request.role, &mountinfo)?;
    let guest_boot_id = parse_boot_id(&read_bounded(BOOT_ID_PATH, 128, "boot_id")?)?;

    if process_start_ticks("self", process_pid)? != helper_start_ticks
        || process_start_ticks("1", 1)? != init_start_ticks
        || namespace_inode("mnt")? != mount_namespace_id
        || namespace_inode("cgroup")? != cgroup_namespace_id
        || self_cgroup_path()? != cgroup_path_in_self_namespace
        || read_bounded(MOUNTINFO_PATH, MOUNTINFO_LIMIT, "mountinfo")? != mountinfo
        || parse_boot_id(&read_bounded(BOOT_ID_PATH, 128, "boot_id")?)? != guest_boot_id
    {
        return Err(ProbeError::Invalid("self_observation_changed"));
    }

    Ok(SelfStorageObservation {
        wire_mode: "self_storage".to_owned(),
        schema_version: 1,
        operation_id: request.operation_id,
        container_id: request.container_id,
        role: request.role,
        helper_protocol_version: WORKER_PROTOCOL_VERSION,
        helper_source_revision,
        guest_boot_id,
        process_pid,
        process_start_ticks: helper_start_ticks,
        init_start_ticks,
        mount_namespace_id,
        cgroup_namespace_id,
        cgroup_path_in_self_namespace,
        self_leaf_limits,
        attachments,
    })
}

fn source_revision() -> Result<String, ProbeError> {
    let value = option_env!("VELNOR_SOURCE_REVISION")
        .ok_or(ProbeError::Invalid("helper_source_revision_missing"))?;
    if value.len() != 40
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ProbeError::Invalid("helper_source_revision"));
    }
    Ok(value.to_owned())
}

fn observe_leaf(path: &str, pid: u32) -> Result<SelfLeafLimits, ProbeError> {
    let directory = cgroup_directory(path)?;
    let facts = ProbeCgroup::parse(
        path,
        &read_cgroup_file(&directory, "memory.max")?,
        &read_cgroup_file(&directory, "memory.current")?,
        &read_cgroup_file(&directory, "memory.swap.max")?,
        &read_cgroup_file(&directory, "memory.swap.current")?,
        &read_cgroup_file(&directory, "cpu.max")?,
        &read_cgroup_file(&directory, "pids.max")?,
    )?;
    let cpuset = parse_cpuset(&read_cgroup_file(&directory, "cpuset.cpus.effective")?)?;
    let pids_current = parse_single_u64(
        &read_cgroup_file(&directory, "pids.current")?,
        "pids_current",
    )?;
    verify_process_membership(path, pid)?;
    Ok(SelfLeafLimits {
        cgroup_path: facts.path,
        memory_max_bytes: facts.memory_max_bytes,
        memory_current_bytes: facts.memory_current_bytes,
        swap_max_bytes: facts.swap_max_bytes,
        swap_current_bytes: facts.swap_current_bytes,
        cpu_quota_micros: facts.cpu_quota_micros,
        cpu_period_micros: facts.cpu_period_micros,
        cpuset_cpus: cpuset.into_iter().collect(),
        pids_max: facts.pids_max,
        pids_current,
    })
}

fn self_cgroup_path() -> Result<String, ProbeError> {
    let bytes = read_bounded(CGROUP_PATH, PROC_LIMIT, "proc_cgroup")?;
    let text = std::str::from_utf8(&bytes).map_err(|_| ProbeError::Invalid("proc_cgroup_utf8"))?;
    let mut result = None;
    for line in text.lines() {
        let Some((hierarchy, rest)) = line.split_once(':') else {
            return Err(ProbeError::Invalid("proc_cgroup_format"));
        };
        let Some((controllers, path)) = rest.split_once(':') else {
            return Err(ProbeError::Invalid("proc_cgroup_format"));
        };
        if hierarchy == "0" && controllers.is_empty() && result.replace(path.to_owned()).is_some() {
            return Err(ProbeError::Invalid("proc_cgroup_duplicate"));
        }
    }
    let path = result.ok_or(ProbeError::Invalid("proc_cgroup_v2_missing"))?;
    validate_cgroup_path(&path)?;
    Ok(path)
}

fn validate_cgroup_path(path: &str) -> Result<(), ProbeError> {
    if path.len() > 256
        || !path.starts_with('/')
        || path.contains("//")
        || path.split('/').any(|part| {
            part == ".."
                || part == "."
                || part.bytes().any(|byte| {
                    !(byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
                })
        })
    {
        return Err(ProbeError::Invalid("self_cgroup_path"));
    }
    Ok(())
}

fn cgroup_directory(path: &str) -> Result<PathBuf, ProbeError> {
    validate_cgroup_path(path)?;
    let relative = path
        .strip_prefix('/')
        .ok_or(ProbeError::Invalid("self_cgroup_path"))?;
    Ok(PathBuf::from(CGROUP_ROOT).join(relative))
}

fn process_start_ticks(pid: &str, expected_pid: u32) -> Result<u64, ProbeError> {
    let input = read_bounded(&format!("/proc/{pid}/stat"), PROC_LIMIT, "proc_stat")?;
    let text = std::str::from_utf8(&input).map_err(|_| ProbeError::Invalid("proc_stat_utf8"))?;
    let Some(close) = text.rfind(')') else {
        return Err(ProbeError::Invalid("proc_stat_format"));
    };
    let pid_field = text[..text
        .find(' ')
        .ok_or(ProbeError::Invalid("proc_stat_format"))?]
        .parse::<u32>()
        .map_err(|_| ProbeError::Invalid("proc_stat_pid"))?;
    if pid_field != expected_pid {
        return Err(ProbeError::Invalid("proc_stat_pid_mismatch"));
    }
    let fields = text[close + 1..]
        .split_ascii_whitespace()
        .collect::<Vec<_>>();
    let value = fields
        .get(19)
        .ok_or(ProbeError::Invalid("proc_stat_starttime"))?;
    parse_decimal(value, "proc_stat_starttime")
}

fn namespace_inode(name: &str) -> Result<u64, ProbeError> {
    let link = fs::read_link(format!("/proc/self/ns/{name}"))
        .map_err(|error| ProbeError::Read("proc_namespace", error))?;
    let text = link
        .to_str()
        .ok_or(ProbeError::Invalid("proc_namespace_format"))?;
    let number = text
        .strip_prefix(&format!("{name}:["))
        .and_then(|value| value.strip_suffix(']'))
        .ok_or(ProbeError::Invalid("proc_namespace_format"))?;
    parse_decimal(number, "proc_namespace_inode")
}

fn read_cgroup_file(
    directory: &std::path::Path,
    name: &'static str,
) -> Result<Vec<u8>, ProbeError> {
    read_bounded(
        &directory.join(name).to_string_lossy(),
        CGROUP_FILE_LIMIT,
        name,
    )
}

fn read_bounded(path: &str, limit: usize, name: &'static str) -> Result<Vec<u8>, ProbeError> {
    let file = fs::File::open(path).map_err(|error| ProbeError::Read(name, error))?;
    let mut bytes = Vec::with_capacity(limit.min(4096));
    file.take(u64::try_from(limit + 1).map_err(|_| ProbeError::Overflow("input_limit"))?)
        .read_to_end(&mut bytes)
        .map_err(|error| ProbeError::Read(name, error))?;
    if bytes.len() > limit {
        return Err(ProbeError::InputTooLarge(name));
    }
    Ok(bytes)
}

fn parse_single_u64(input: &[u8], error: &'static str) -> Result<u64, ProbeError> {
    let text = std::str::from_utf8(input).map_err(|_| ProbeError::Invalid(error))?;
    parse_decimal(text.trim(), error)
}

fn parse_decimal(value: &str, error: &'static str) -> Result<u64, ProbeError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ProbeError::Invalid(error));
    }
    value.parse().map_err(|_| ProbeError::Overflow(error))
}
