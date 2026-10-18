use std::io::Read;

use crate::{
    ProbeError,
    worker_binding::{
        CgroupNodeObservation, OwnedContainerCgroupObservation, ProcessWitness, WorkerRole,
    },
};

use super::cgroup::{observe_hierarchy, verify_membership};

const PROC_FILE_LIMIT: usize = 4096;
const MAX_NAMESPACE_DEPTH: usize = 8;

pub(super) fn observe_container(
    role: WorkerRole,
    container_id: &str,
    init_pid: u32,
    init_start_ticks: u64,
    helper_pid: u32,
    helper_start_ticks: u64,
) -> Result<OwnedContainerCgroupObservation, ProbeError> {
    if init_pid == helper_pid || init_pid == 0 || helper_pid == 0 {
        return Err(ProbeError::Invalid("owned_process_identity"));
    }
    let init = observe_process(init_pid, init_start_ticks)?;
    let helper = observe_process(helper_pid, helper_start_ticks)?;
    if init.cgroup_path != helper.cgroup_path || init.namespace_pids.last() != Some(&1) {
        return Err(ProbeError::Invalid("owned_process_cgroup_mismatch"));
    }
    let hierarchy = observe_hierarchy(&init.cgroup_path)?;
    verify_membership(&init.cgroup_path, init.host_pid)?;
    verify_membership(&helper.cgroup_path, helper.host_pid)?;
    let hierarchy_after = observe_hierarchy(&init.cgroup_path)?;
    if !same_limits(&hierarchy, &hierarchy_after) {
        return Err(ProbeError::Invalid("owned_cgroup_limits_changed"));
    }
    let init_after = observe_process(init_pid, init_start_ticks)?;
    let helper_after = observe_process(helper_pid, helper_start_ticks)?;
    if init_after != init || helper_after != helper {
        return Err(ProbeError::Invalid("owned_process_changed"));
    }
    Ok(OwnedContainerCgroupObservation {
        role,
        container_id: container_id.to_owned(),
        init,
        helper,
        hierarchy,
    })
}

fn observe_process(pid: u32, expected_start_ticks: u64) -> Result<ProcessWitness, ProbeError> {
    let start_before = process_start_ticks(pid)?;
    let pids_before = namespace_pids(pid)?;
    let cgroup_path = process_cgroup_path(pid)?;
    if start_before != expected_start_ticks || pids_before.first() != Some(&pid) {
        return Err(ProbeError::Invalid("owned_process_identity_mismatch"));
    }
    verify_membership(&cgroup_path, pid)?;
    let start_after = process_start_ticks(pid)?;
    let namespace_after = namespace_pids(pid)?;
    let cgroup_after = process_cgroup_path(pid)?;
    if start_after != start_before || namespace_after != pids_before || cgroup_after != cgroup_path
    {
        return Err(ProbeError::Invalid("owned_process_changed"));
    }
    Ok(ProcessWitness {
        host_pid: pid,
        start_ticks: start_before,
        namespace_pids: pids_before,
        cgroup_path,
    })
}

fn process_start_ticks(pid: u32) -> Result<u64, ProbeError> {
    let bytes = read_proc(pid, "stat")?;
    parse_stat_start_ticks(&bytes, pid)
}

fn parse_stat_start_ticks(input: &[u8], expected_pid: u32) -> Result<u64, ProbeError> {
    if input.len() > PROC_FILE_LIMIT {
        return Err(ProbeError::InputTooLarge("proc_stat"));
    }
    let text = std::str::from_utf8(input).map_err(|_| ProbeError::Invalid("proc_stat_utf8"))?;
    let Some(close) = text.rfind(')') else {
        return Err(ProbeError::Invalid("proc_stat_format"));
    };
    let fields = text[close + 1..]
        .split_ascii_whitespace()
        .collect::<Vec<_>>();
    let pid_field = text[..text
        .find(' ')
        .ok_or(ProbeError::Invalid("proc_stat_format"))?]
        .parse::<u32>()
        .map_err(|_| ProbeError::Invalid("proc_stat_pid"))?;
    if pid_field != expected_pid {
        return Err(ProbeError::Invalid("proc_stat_pid_mismatch"));
    }
    parse_decimal(
        fields
            .get(19)
            .ok_or(ProbeError::Invalid("proc_stat_starttime"))?,
        "proc_stat_starttime",
    )
}

fn namespace_pids(pid: u32) -> Result<Vec<u32>, ProbeError> {
    let bytes = read_proc(pid, "status")?;
    parse_namespace_pids(&bytes, pid)
}

fn parse_namespace_pids(bytes: &[u8], pid: u32) -> Result<Vec<u32>, ProbeError> {
    if bytes.len() > PROC_FILE_LIMIT {
        return Err(ProbeError::InputTooLarge("proc_status"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| ProbeError::Invalid("proc_status_utf8"))?;
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
            .map(|part| parse_pid(part, "proc_status_nspid"))
            .collect::<Result<Vec<_>, _>>()?;
        if values.is_empty()
            || values.len() > MAX_NAMESPACE_DEPTH
            || values[0] != pid
            || values.contains(&0)
        {
            return Err(ProbeError::Invalid("proc_status_nspid"));
        }
        result = Some(values);
    }
    result.ok_or(ProbeError::Invalid("proc_status_nspid_missing"))
}

fn process_cgroup_path(pid: u32) -> Result<String, ProbeError> {
    let bytes = read_proc(pid, "cgroup")?;
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
    super::cgroup::validate_cgroup_path(&path)?;
    Ok(path)
}

fn read_proc(pid: u32, name: &'static str) -> Result<Vec<u8>, ProbeError> {
    let path = format!("/proc/{pid}/{name}");
    let file = std::fs::File::open(path).map_err(|error| ProbeError::Read(name, error))?;
    let mut bytes = Vec::with_capacity(1024);
    file.take((PROC_FILE_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| ProbeError::Read(name, error))?;
    if bytes.len() > PROC_FILE_LIMIT {
        return Err(ProbeError::InputTooLarge(name));
    }
    Ok(bytes)
}

fn parse_pid(value: &str, error: &'static str) -> Result<u32, ProbeError> {
    let parsed = parse_decimal(value, error)?;
    u32::try_from(parsed).map_err(|_| ProbeError::Overflow(error))
}

fn parse_decimal(value: &str, error: &'static str) -> Result<u64, ProbeError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ProbeError::Invalid(error));
    }
    value.parse().map_err(|_| ProbeError::Overflow(error))
}

fn same_limits(left: &[CgroupNodeObservation], right: &[CgroupNodeObservation]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.path == right.path
                && left.root == right.root
                && left.memory_max_bytes == right.memory_max_bytes
                && left.swap_max_bytes == right.swap_max_bytes
                && left.cpu_quota_micros == right.cpu_quota_micros
                && left.cpu_period_micros == right.cpu_period_micros
                && left.cpuset_cpus == right.cpuset_cpus
                && left.pids_max == right.pids_max
        })
}

#[cfg(test)]
mod tests {
    use super::{parse_namespace_pids, parse_stat_start_ticks};

    #[test]
    fn stat_parser_handles_parentheses_and_checks_the_pid() {
        let row = b"42 (worker (helper)) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 12345 22\n";
        assert!(parse_stat_start_ticks(row, 42).is_ok_and(|ticks| ticks == 12345));
        assert!(parse_stat_start_ticks(row, 41).is_err());
        assert!(parse_stat_start_ticks(b"42 worker S 1", 42).is_err());
    }

    #[test]
    fn nspid_parser_keeps_repeated_values_across_namespaces() {
        assert!(
            parse_namespace_pids(b"Name:\tworker\nNSpid:\t810 42 1\n", 810)
                .is_ok_and(|pids| pids == [810, 42, 1])
        );
        assert!(
            parse_namespace_pids(b"NSpid:\t810 810 1\n", 810)
                .is_ok_and(|pids| pids == [810, 810, 1])
        );
        assert!(parse_namespace_pids(b"NSpid:\t9 9\n", 9).is_ok());
        assert!(parse_namespace_pids(b"NSpid:\t9 0\n", 9).is_err());
        assert!(parse_namespace_pids(b"NSpid:\t10\n", 9).is_err());
        assert!(parse_namespace_pids(b"NSpid:\t1 2 3 4 5 6 7 8 9\n", 1).is_err());
    }
}
