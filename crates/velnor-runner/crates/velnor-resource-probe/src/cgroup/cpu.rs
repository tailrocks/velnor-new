use std::{
    collections::BTreeSet,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use crate::{ProbeError, filesystem::parse_u64, record::CpuAncestorLimit, units::digits};

const CGROUP_ROOT: &str = "/sys/fs/cgroup";
const MAX_CGROUP_DEPTH: usize = 16;
const MAX_CGROUP_PATH_BYTES: usize = 256;
const MAX_CPU_ID: u32 = 65_535;
const MAX_EFFECTIVE_CPUS: usize = 4096;
const CGROUP_FILE_LIMIT: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CpuHierarchy {
    pub(crate) leaf_cpuset_cpus_count: u32,
    pub(crate) ancestors: Vec<CpuAncestorLimit>,
    pub(crate) leaf_quota_micros: Option<u64>,
    pub(crate) leaf_period_micros: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NamespacePids {
    pub(crate) host_pid: u32,
    pub(crate) namespace_pid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CpuLimit {
    quota_micros: Option<u64>,
    period_micros: u64,
}

/// Read the selected process's full visible cgroup-v2 CPU ancestry.
pub(crate) fn observe_hierarchy(path: &str) -> Result<CpuHierarchy, ProbeError> {
    let paths = hierarchy_paths(path)?;
    let mut previous_cpus: Option<BTreeSet<u32>> = None;
    let mut cpus_by_path = Vec::with_capacity(paths.len());
    let mut limits_by_path = Vec::with_capacity(paths.len());

    for path in &paths {
        let directory = cgroup_directory(path)?;
        let cpus = parse_cpuset(&read_cgroup_file(&directory, "cpuset.cpus.effective")?)?;
        if previous_cpus
            .as_ref()
            .is_some_and(|parent| !cpus.is_subset(parent))
        {
            return Err(ProbeError::Invalid("cgroup_cpuset_not_subset"));
        }
        previous_cpus = Some(cpus.clone());
        cpus_by_path.push(cpus);
        limits_by_path.push(parse_cpu_limit(&read_cgroup_file(&directory, "cpu.max")?)?);
    }

    let leaf_cpus = cpus_by_path
        .pop()
        .ok_or(ProbeError::Invalid("cgroup_cpu_leaf_missing"))?;
    let leaf_limit = limits_by_path
        .pop()
        .ok_or(ProbeError::Invalid("cgroup_cpu_leaf_missing"))?;
    let mut ancestors = Vec::with_capacity(paths.len().saturating_sub(1));
    for (path, cpus, limit) in paths
        .into_iter()
        .take(cpus_by_path.len())
        .zip(cpus_by_path)
        .zip(limits_by_path)
        .map(|((path, cpus), limit)| (path, cpus, limit))
    {
        ancestors.push(CpuAncestorLimit {
            path,
            cpuset_cpus_count: u32::try_from(cpus.len())
                .map_err(|_| ProbeError::Overflow("cgroup_cpuset_count"))?,
            cpu_quota_micros: limit.quota_micros,
            cpu_period_micros: limit.period_micros,
        });
    }
    Ok(CpuHierarchy {
        leaf_cpuset_cpus_count: u32::try_from(leaf_cpus.len())
            .map_err(|_| ProbeError::Overflow("cgroup_cpuset_count"))?,
        ancestors,
        leaf_quota_micros: leaf_limit.quota_micros,
        leaf_period_micros: leaf_limit.period_micros,
    })
}

pub(crate) fn hierarchy_paths(path: &str) -> Result<Vec<String>, ProbeError> {
    if path.is_empty()
        || path.len() > MAX_CGROUP_PATH_BYTES
        || !path.starts_with('/')
        || path.contains("//")
        || path.chars().any(char::is_control)
    {
        return Err(ProbeError::Invalid("probe_cgroup_path"));
    }
    let components = if path == "/" {
        Vec::new()
    } else {
        path.split('/').skip(1).collect::<Vec<_>>()
    };
    if components.iter().any(|part| {
        part.is_empty()
            || *part == "."
            || *part == ".."
            || part.contains('\\')
            || part.contains(':')
    }) || components.len() > MAX_CGROUP_DEPTH
    {
        return Err(ProbeError::Invalid("probe_cgroup_path"));
    }
    let mut paths = Vec::with_capacity(components.len() + 1);
    let mut current = String::from("/");
    paths.push(current.clone());
    for component in components {
        if current != "/" {
            current.push('/');
        }
        current.push_str(component);
        paths.push(current.clone());
    }
    Ok(paths)
}

pub(crate) fn namespace_pids(status: &[u8], current_pid: u32) -> Result<NamespacePids, ProbeError> {
    let text = std::str::from_utf8(status).map_err(|_| ProbeError::Invalid("proc_status_utf8"))?;
    let mut result = None;
    for line in text.lines() {
        let Some(value) = line.strip_prefix("NSpid:") else {
            continue;
        };
        if result.is_some() {
            return Err(ProbeError::Invalid("proc_status_nspid_duplicate"));
        }
        let values = value
            .split_ascii_whitespace()
            .map(parse_pid)
            .collect::<Result<Vec<_>, _>>()?;
        let (Some(host_pid), Some(namespace_pid)) = (values.first(), values.last()) else {
            return Err(ProbeError::Invalid("proc_status_nspid_missing"));
        };
        // The trusted host agent creates this container with PIDMode=host.
        // A nested PID namespace would make its leaf `cgroup.procs` PID
        // incomparable with Docker inspect's State.Pid.
        if values.len() != 1 || host_pid != namespace_pid || *namespace_pid != current_pid {
            return Err(ProbeError::Invalid("proc_status_nspid_mismatch"));
        }
        result = Some(NamespacePids {
            host_pid: *host_pid,
            namespace_pid: *namespace_pid,
        });
    }
    result.ok_or(ProbeError::Invalid("proc_status_nspid_missing"))
}

pub(crate) fn verify_process_membership(path: &str, process_pid: u32) -> Result<(), ProbeError> {
    let directory = cgroup_directory(path)?;
    let processes = read_cgroup_file(&directory, "cgroup.procs")?;
    verify_process_list(&processes, process_pid)
}

fn verify_process_list(processes: &[u8], process_pid: u32) -> Result<(), ProbeError> {
    let text =
        std::str::from_utf8(processes).map_err(|_| ProbeError::Invalid("cgroup_procs_utf8"))?;
    let mut found = false;
    let mut seen = BTreeSet::new();
    for line in text.lines() {
        let pid = parse_pid(line.trim())?;
        if !seen.insert(pid) {
            return Err(ProbeError::Invalid("cgroup_procs_duplicate"));
        }
        found |= pid == process_pid;
    }
    if !found {
        return Err(ProbeError::Invalid("cgroup_process_missing"));
    }
    Ok(())
}

fn parse_pid(value: &str) -> Result<u32, ProbeError> {
    if !digits(value) {
        return Err(ProbeError::Invalid("pid_format"));
    }
    let pid = value
        .parse::<u32>()
        .map_err(|_| ProbeError::Overflow("pid"))?;
    (pid > 0)
        .then_some(pid)
        .ok_or(ProbeError::Invalid("pid_zero"))
}

#[cfg(test)]
mod namespace_tests {
    use super::{namespace_pids, verify_process_list};

    #[test]
    fn host_pid_namespace_requires_one_unambiguous_pid() {
        let pids = namespace_pids(b"Name:\tprobe\nNSpid:\t4321\n", 4321);
        assert!(pids.is_ok_and(|pids| pids.host_pid == 4321 && pids.namespace_pid == 4321));
        assert!(namespace_pids(b"Name:\tprobe\nNSpid:\t4321 1\n", 1).is_err());
        assert!(namespace_pids(b"Name:\tprobe\nNSpid:\t4321\n", 1).is_err());
    }

    #[test]
    fn exact_cgroup_membership_requires_the_inspected_host_pid() {
        assert!(verify_process_list(b"4321\n7777\n", 4321).is_ok());
        assert!(verify_process_list(b"1\n7777\n", 4321).is_err());
        assert!(verify_process_list(b"4321\n4321\n", 4321).is_err());
    }
}

pub(crate) fn parse_cpuset(input: &[u8]) -> Result<BTreeSet<u32>, ProbeError> {
    let text = std::str::from_utf8(input).map_err(|_| ProbeError::Invalid("cpuset_utf8"))?;
    let text = text.trim();
    if text.is_empty() || text.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return Err(ProbeError::Invalid("cpuset_format"));
    }
    let mut cpus = BTreeSet::new();
    for item in text.split(',') {
        let (first, last) = match item.split_once('-') {
            Some((first, last)) if !last.contains('-') => (cpu_id(first)?, cpu_id(last)?),
            Some(_) => return Err(ProbeError::Invalid("cpuset_range")),
            None => {
                let cpu = cpu_id(item)?;
                (cpu, cpu)
            }
        };
        if last < first {
            return Err(ProbeError::Invalid("cpuset_range"));
        }
        let span =
            usize::try_from(last - first).map_err(|_| ProbeError::Overflow("cpuset_range"))?;
        if span >= MAX_EFFECTIVE_CPUS {
            return Err(ProbeError::Invalid("cpuset_range"));
        }
        for cpu in first..=last {
            if !cpus.insert(cpu) || cpus.len() > MAX_EFFECTIVE_CPUS {
                return Err(ProbeError::Invalid("cpuset_duplicate_or_large"));
            }
        }
    }
    if cpus.is_empty() {
        return Err(ProbeError::Invalid("cpuset_empty"));
    }
    Ok(cpus)
}

fn cpu_id(value: &str) -> Result<u32, ProbeError> {
    if !digits(value) {
        return Err(ProbeError::Invalid("cpuset_cpu_id"));
    }
    let cpu = value
        .parse::<u32>()
        .map_err(|_| ProbeError::Overflow("cpuset_cpu_id"))?;
    if cpu > MAX_CPU_ID {
        return Err(ProbeError::Invalid("cpuset_cpu_id"));
    }
    Ok(cpu)
}

fn parse_cpu_limit(input: &[u8]) -> Result<CpuLimit, ProbeError> {
    let text = std::str::from_utf8(input).map_err(|_| ProbeError::Invalid("cpu_max_utf8"))?;
    let mut fields = text.split_ascii_whitespace();
    let quota = fields.next().ok_or(ProbeError::Invalid("cpu_max_format"))?;
    let period = fields.next().ok_or(ProbeError::Invalid("cpu_max_format"))?;
    if fields.next().is_some() || !digits(period) {
        return Err(ProbeError::Invalid("cpu_max_format"));
    }
    let period_micros = parse_u64(period, "cpu_period")?;
    if period_micros == 0 {
        return Err(ProbeError::Invalid("cpu_period"));
    }
    let quota_micros = if quota == "max" {
        None
    } else {
        let quota = parse_u64(quota, "cpu_quota")?;
        if quota == 0 {
            return Err(ProbeError::Invalid("cpu_quota"));
        }
        Some(quota)
    };
    Ok(CpuLimit {
        quota_micros,
        period_micros,
    })
}

fn cgroup_directory(path: &str) -> Result<PathBuf, ProbeError> {
    let relative = path
        .strip_prefix('/')
        .ok_or(ProbeError::Invalid("probe_cgroup_path"))?;
    let directory = Path::new(CGROUP_ROOT).join(relative);
    if !directory.starts_with(CGROUP_ROOT) {
        return Err(ProbeError::Invalid("probe_cgroup_path"));
    }
    Ok(directory)
}

fn read_cgroup_file(directory: &Path, name: &'static str) -> Result<Vec<u8>, ProbeError> {
    let file = File::open(directory.join(name)).map_err(|error| ProbeError::Read(name, error))?;
    let mut bytes = Vec::with_capacity(128);
    file.take((CGROUP_FILE_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| ProbeError::Read(name, error))?;
    if bytes.len() > CGROUP_FILE_LIMIT {
        return Err(ProbeError::InputTooLarge(name));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_root_to_parent_paths_and_excludes_leaf() -> Result<(), ProbeError> {
        assert_eq!(
            hierarchy_paths("/docker/scope/runner")?,
            ["/", "/docker", "/docker/scope", "/docker/scope/runner"]
        );
        assert_eq!(hierarchy_paths("/")?, ["/"]);
        Ok(())
    }

    #[test]
    fn rejects_unsafe_and_excessively_deep_paths() {
        assert!(hierarchy_paths("/a/../b").is_err());
        assert!(hierarchy_paths("/a//b").is_err());
        assert!(hierarchy_paths(&format!("/{}", vec!["x"; 17].join("/"))).is_err());
    }

    #[test]
    fn parses_bounded_cpuset_ranges_and_rejects_duplicates() -> Result<(), ProbeError> {
        assert_eq!(parse_cpuset(b"0-3,8\n")?, BTreeSet::from([0, 1, 2, 3, 8]));
        assert!(parse_cpuset(b"0-2,2").is_err());
        assert!(parse_cpuset(b"2-1").is_err());
        assert!(parse_cpuset(b"0,,1").is_err());
        assert!(parse_cpuset(b"0-99999").is_err());
        Ok(())
    }

    #[test]
    fn parses_finite_fractional_and_unlimited_cpu_limits() -> Result<(), ProbeError> {
        assert_eq!(
            parse_cpu_limit(b"max 100000\n")?,
            CpuLimit {
                quota_micros: None,
                period_micros: 100_000,
            }
        );
        assert_eq!(
            parse_cpu_limit(b"250000 100000\n")?,
            CpuLimit {
                quota_micros: Some(250_000),
                period_micros: 100_000,
            }
        );
        assert!(parse_cpu_limit(b"100000 0").is_err());
        assert!(parse_cpu_limit(b"0 100000").is_err());
        assert!(parse_cpu_limit(b"max 100000 extra").is_err());
        Ok(())
    }
}
