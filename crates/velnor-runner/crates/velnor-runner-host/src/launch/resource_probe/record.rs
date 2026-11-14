// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) use velnor_runner_qualification::probe_record::{
    InodePolicy, MAX_OUTPUT_BYTES, ProbeRecord,
};

pub(super) fn parse(output: &[u8], memory_total_bytes: u64) -> Option<ProbeRecord> {
    if memory_total_bytes == 0 {
        return None;
    }
    let record = ProbeRecord::parse_schema5(output)?;
    let (expected_period, expected_quota, _, _, _) = super::projection::resource_limits();
    (record.memory_available_bytes <= memory_total_bytes
        && record.probe_cgroup_cpu_period_micros == u64::try_from(expected_period).ok()?
        && record.probe_cgroup_cpu_quota_micros == Some(u64::try_from(expected_quota).ok()?))
    .then_some(record)
}

pub(super) fn effective_cpu_millicores(record: &ProbeRecord, ncpu: u32) -> Option<u64> {
    if ncpu == 0 || record.probe_cgroup_cpuset_cpus_count == 0 {
        return None;
    }
    let (expected_period, expected_quota, _, _, _) = super::projection::resource_limits();
    if record.probe_cgroup_cpu_period_micros != u64::try_from(expected_period).ok()?
        || record.probe_cgroup_cpu_quota_micros != Some(u64::try_from(expected_quota).ok()?)
    {
        return None;
    }
    record.ancestor_capacity_millicores(ncpu)
}
