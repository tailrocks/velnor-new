// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) use crate::resource_probe_record::{InodePolicy, MAX_OUTPUT_BYTES, ProbeRecord};

impl ProbeRecord {
    pub(super) fn parse(output: &[u8], memory_total_bytes: u64) -> Option<Self> {
        if memory_total_bytes == 0 {
            return None;
        }
        let record = Self::parse_schema5(output)?;
        let (expected_period, expected_quota, _, _, _) = super::projection::resource_limits();
        (record.docker_root_memory_values_valid(memory_total_bytes)
            && record.probe_cgroup_cpu_period_micros == u64::try_from(expected_period).ok()?
            && record.probe_cgroup_cpu_quota_micros == Some(u64::try_from(expected_quota).ok()?))
        .then_some(record)
    }

    fn docker_root_memory_values_valid(&self, memory_total_bytes: u64) -> bool {
        self.memory_available_bytes <= memory_total_bytes
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
        self.ancestor_capacity_millicores(ncpu)
    }
}
