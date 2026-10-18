use std::{fs::File, io::Read, path::PathBuf};

use crate::{
    ProbeError,
    cgroup::parse_cpuset,
    worker_binding::{CgroupNodeObservation, MAX_CGROUP_NODES},
};

const CGROUP_ROOT: &str = "/sys/fs/cgroup";
const CGROUP_FILE_LIMIT: usize = 4096;

pub(super) fn observe_hierarchy(path: &str) -> Result<Vec<CgroupNodeObservation>, ProbeError> {
    let paths = hierarchy_paths(path)?;
    paths
        .iter()
        .enumerate()
        .map(|(index, path)| observe_node(path, index == 0))
        .collect()
}

pub(super) fn verify_membership(path: &str, pid: u32) -> Result<(), ProbeError> {
    let directory = cgroup_directory(path)?;
    let bytes = read_required(&directory.join("cgroup.procs"), "cgroup_procs")?;
    let text = std::str::from_utf8(&bytes).map_err(|_| ProbeError::Invalid("cgroup_procs_utf8"))?;
    let mut found = false;
    let mut seen = std::collections::BTreeSet::new();
    for line in text.lines() {
        let current = parse_u32(line, "cgroup_procs_pid")?;
        if !seen.insert(current) {
            return Err(ProbeError::Invalid("cgroup_procs_duplicate"));
        }
        found |= current == pid;
    }
    if found {
        Ok(())
    } else {
        Err(ProbeError::Invalid("cgroup_process_missing"))
    }
}

pub(super) fn validate_cgroup_path(path: &str) -> Result<(), ProbeError> {
    if path == "/" {
        return Ok(());
    }
    if path.len() > 256
        || !path.starts_with('/')
        || path.contains("//")
        || path.split('/').skip(1).any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.bytes().any(|byte| {
                    !(byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
                })
        })
    {
        return Err(ProbeError::Invalid("owned_cgroup_path"));
    }
    Ok(())
}

fn hierarchy_paths(path: &str) -> Result<Vec<String>, ProbeError> {
    validate_cgroup_path(path)?;
    let components = if path == "/" {
        Vec::new()
    } else {
        path.split('/').skip(1).collect::<Vec<_>>()
    };
    if components.len() + 1 > MAX_CGROUP_NODES {
        return Err(ProbeError::Invalid("owned_cgroup_depth"));
    }
    let mut paths = vec!["/".to_owned()];
    let mut current = String::new();
    for component in components {
        current.push('/');
        current.push_str(component);
        paths.push(current.clone());
    }
    Ok(paths)
}

fn observe_node(path: &str, root: bool) -> Result<CgroupNodeObservation, ProbeError> {
    let directory = cgroup_directory(path)?;
    let memory = read_optional(&directory.join("memory.max"), root)?;
    let memory_current = read_optional(&directory.join("memory.current"), root)?;
    let swap = read_optional(&directory.join("memory.swap.max"), root)?;
    let swap_current = read_optional(&directory.join("memory.swap.current"), root)?;
    let cpu = read_optional(&directory.join("cpu.max"), root)?;
    let cpuset = read_optional(&directory.join("cpuset.cpus.effective"), root)?;
    let pids = read_optional(&directory.join("pids.max"), root)?;
    let pids_current = read_optional(&directory.join("pids.current"), root)?;
    let (cpu_quota_micros, cpu_period_micros) = match cpu {
        Some(value) => parse_cpu(&value)?,
        None if root => (None, None),
        None => return Err(ProbeError::Invalid("owned_cgroup_cpu_missing")),
    };
    Ok(CgroupNodeObservation {
        path: path.to_owned(),
        root,
        memory_max_bytes: parse_optional_limit(memory.as_deref(), root, "memory.max")?,
        memory_current_bytes: parse_optional_value(
            memory_current.as_deref(),
            root,
            "memory.current",
        )?,
        swap_max_bytes: parse_optional_limit(swap.as_deref(), root, "memory.swap.max")?,
        swap_current_bytes: parse_optional_value(
            swap_current.as_deref(),
            root,
            "memory.swap.current",
        )?,
        cpu_quota_micros,
        cpu_period_micros,
        cpuset_cpus: match cpuset {
            Some(value) => Some(parse_cpuset(&value)?.into_iter().collect()),
            None if root => None,
            None => return Err(ProbeError::Invalid("owned_cgroup_cpuset_missing")),
        },
        pids_max: parse_optional_limit(pids.as_deref(), root, "pids.max")?,
        pids_current: parse_optional_value(pids_current.as_deref(), root, "pids.current")?,
    })
}

fn cgroup_directory(path: &str) -> Result<PathBuf, ProbeError> {
    validate_cgroup_path(path)?;
    let relative = path
        .strip_prefix('/')
        .ok_or(ProbeError::Invalid("owned_cgroup_path"))?;
    Ok(PathBuf::from(CGROUP_ROOT).join(relative))
}

fn read_optional(path: &std::path::Path, root: bool) -> Result<Option<Vec<u8>>, ProbeError> {
    match read_required(path, "cgroup_control") {
        Ok(bytes) => Ok(Some(bytes)),
        Err(ProbeError::Read(_, error)) if root && error.kind() == std::io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn read_required(path: &std::path::Path, name: &'static str) -> Result<Vec<u8>, ProbeError> {
    let file = File::open(path).map_err(|error| ProbeError::Read(name, error))?;
    let mut bytes = Vec::with_capacity(128);
    file.take((CGROUP_FILE_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| ProbeError::Read(name, error))?;
    if bytes.len() > CGROUP_FILE_LIMIT {
        return Err(ProbeError::InputTooLarge(name));
    }
    Ok(bytes)
}

fn parse_optional_limit(
    input: Option<&[u8]>,
    root: bool,
    name: &'static str,
) -> Result<Option<u64>, ProbeError> {
    let Some(input) = input else {
        if root {
            return Ok(None);
        }
        return Err(ProbeError::Invalid("owned_cgroup_control_missing"));
    };
    parse_limit(input, name)
}

fn parse_optional_value(
    input: Option<&[u8]>,
    root: bool,
    name: &'static str,
) -> Result<Option<u64>, ProbeError> {
    let Some(input) = input else {
        if root {
            return Ok(None);
        }
        return Err(ProbeError::Invalid("owned_cgroup_control_missing"));
    };
    Ok(Some(parse_decimal(input, name)?))
}

fn parse_limit(input: &[u8], name: &'static str) -> Result<Option<u64>, ProbeError> {
    let value = token(input, name)?;
    if value == "max" {
        Ok(None)
    } else {
        parse_decimal(input, name).map(Some)
    }
}

fn parse_cpu(input: &[u8]) -> Result<(Option<u64>, Option<u64>), ProbeError> {
    let text = std::str::from_utf8(input).map_err(|_| ProbeError::Invalid("cpu_max_utf8"))?;
    let mut fields = text.split_ascii_whitespace();
    let quota = fields.next().ok_or(ProbeError::Invalid("cpu_max_format"))?;
    let period = fields.next().ok_or(ProbeError::Invalid("cpu_max_format"))?;
    if fields.next().is_some() {
        return Err(ProbeError::Invalid("cpu_max_format"));
    }
    let period = parse_decimal(period.as_bytes(), "cpu_period")?;
    if period == 0 {
        return Err(ProbeError::Invalid("cpu_period"));
    }
    let quota = if quota == "max" {
        None
    } else {
        let value = parse_decimal(quota.as_bytes(), "cpu_quota")?;
        if value == 0 {
            return Err(ProbeError::Invalid("cpu_quota"));
        }
        Some(value)
    };
    Ok((quota, Some(period)))
}

fn token<'a>(input: &'a [u8], name: &'static str) -> Result<&'a str, ProbeError> {
    let text = std::str::from_utf8(input).map_err(|_| ProbeError::Invalid(name))?;
    let value = text.trim();
    if value.is_empty() || value.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return Err(ProbeError::Invalid(name));
    }
    Ok(value)
}

fn parse_decimal(input: &[u8], name: &'static str) -> Result<u64, ProbeError> {
    let value = token(input, name)?;
    if !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ProbeError::Invalid(name));
    }
    value.parse().map_err(|_| ProbeError::Overflow(name))
}

fn parse_u32(value: &str, name: &'static str) -> Result<u32, ProbeError> {
    let parsed = parse_decimal(value.as_bytes(), name)?;
    u32::try_from(parsed).map_err(|_| ProbeError::Overflow(name))
}

#[cfg(test)]
mod tests {
    use super::{hierarchy_paths, parse_cpu, parse_limit, validate_cgroup_path};

    #[test]
    fn hierarchy_paths_include_root_and_bound_depth() {
        assert!(hierarchy_paths("/").is_ok_and(|paths| paths == ["/".to_owned()]));
        assert!(
            hierarchy_paths("/docker/job/runner").is_ok_and(|paths| paths
                == [
                    "/".to_owned(),
                    "/docker".to_owned(),
                    "/docker/job".to_owned(),
                    "/docker/job/runner".to_owned(),
                ])
        );
        assert!(hierarchy_paths("/a/../b").is_err());
        assert!(hierarchy_paths("/a//b").is_err());
        assert!(hierarchy_paths("/a:b").is_err());
    }

    #[test]
    fn cpu_parser_preserves_quota_period_and_rejects_bad_values() {
        assert!(
            parse_cpu(b"200000 100000\n")
                .is_ok_and(|values| values == (Some(200_000), Some(100_000)))
        );
        assert!(parse_cpu(b"max 100000\n").is_ok_and(|values| values == (None, Some(100_000))));
        assert!(parse_cpu(b"0 100000\n").is_err());
        assert!(parse_cpu(b"100 0\n").is_err());
        assert!(parse_cpu(b"100 100000 extra\n").is_err());
    }

    #[test]
    fn limit_parser_keeps_unlimited_distinct_from_missing() {
        assert!(parse_limit(b"max\n", "limit").is_ok_and(|value| value.is_none()));
        assert!(parse_limit(b"512\n", "limit").is_ok_and(|value| value == Some(512)));
        assert!(parse_limit(b"-1\n", "limit").is_err());
        assert!(parse_limit(b"max extra\n", "limit").is_err());
        assert!(validate_cgroup_path("/docker/job.scope").is_ok());
        assert!(validate_cgroup_path("/docker/../host").is_err());
    }
}
