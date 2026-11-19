// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

use velnor_runner_qualification::probe_record::CpuAncestorLimit;

use super::{Observation, ObservationInput, retained_tmpfs_bytes};
use crate::launch::resource_probe::execute::{ProbeCleanupEvidence, ProbeStartupEvidence};
use crate::launch::resource_probe::provider::ProbeImagePreparationEvidence;
use crate::launch::resource_probe::record::{InodePolicy, ProbeRecord};
use crate::worker::{ResourceBudget, ResourceBudgetConfig};

const GIB: u64 = 1 << 30;
const OPERATION_ID: &str = "0123456789abcdef0123456789abcdef";
const ENGINE_ID: &str = "engine-test";
const ROOT_DIGEST: &str = "1111111111111111111111111111111111111111111111111111111111111111";

fn budget() -> Option<ResourceBudget> {
    ResourceBudgetConfig {
        runner_cpu_millicores: 2_000,
        runner_memory_bytes: 3 * GIB,
        dind_cpu_millicores: 2_000,
        dind_memory_bytes: 5 * GIB,
        operational_memory_reserve_bytes: GIB,
    }
    .validate()
    .ok()
}

fn record(memory_available_bytes: u64) -> ProbeRecord {
    ProbeRecord {
        schema_version: 5,
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
        memory_available_bytes,
        load_milli: 250,
        memory_psi_some_avg10_bps: Some(0),
        probe_cgroup_path: "/docker/probe-1".to_owned(),
        probe_cgroup_memory_max_bytes: Some(67_108_864),
        probe_cgroup_memory_current_bytes: 4_194_304,
        probe_cgroup_swap_max_bytes: Some(0),
        probe_cgroup_swap_current_bytes: 0,
        probe_cgroup_cpu_quota_micros: Some(10_000),
        probe_cgroup_cpu_period_micros: 100_000,
        probe_cgroup_cpuset_cpus_count: 8,
        cpu_ancestor_limits: vec![CpuAncestorLimit {
            path: "/".to_owned(),
            cpuset_cpus_count: 8,
            cpu_quota_micros: None,
            cpu_period_micros: 100_000,
        }],
        probe_host_pid: 1234,
        probe_namespace_pid: 1234,
        probe_cgroup_pids_max: Some(64),
    }
}

fn observation(available: u64) -> Option<Observation> {
    Observation::new(ObservationInput {
        record: record(available),
        cpus: 8,
        inspected_host_pid: 1234,
        memory_total_bytes: available.checked_add(GIB)?,
        engine_id: ENGINE_ID.to_owned(),
        docker_root_digest: ROOT_DIGEST.to_owned(),
        sample_generation_id: OPERATION_ID.to_owned(),
        observed_at: Instant::now(),
        startup_evidence: ProbeStartupEvidence::new(
            ProbeCleanupEvidence::for_test(OPERATION_ID, ENGINE_ID, ROOT_DIGEST),
            ProbeImagePreparationEvidence::for_test(ENGINE_ID, ROOT_DIGEST),
        ),
    })
}

fn permit_admits(available: u64) -> Option<bool> {
    let budget = budget()?;
    Some(observation(available)?.into_start_permit(budget).is_some())
}

fn recovery_admits(available: u64) -> Option<bool> {
    let budget = budget()?;
    let charge = (budget.pair().cpu_millicores, budget.pair().memory_bytes);
    Some(
        observation(available)?
            .into_recovery_observation(budget, charge, ENGINE_ID, ROOT_DIGEST)
            .is_some(),
    )
}

fn required() -> Option<u64> {
    let budget = budget()?;
    budget
        .pair()
        .memory_bytes
        .checked_add(retained_tmpfs_bytes()?)?
        .checked_add(budget.admission_reserve().memory_bytes)
}

#[test]
fn memory_exactly_covering_pair_retained_and_reserve_is_admitted_by_both_paths() {
    let available = required().unwrap_or_default();
    assert_ne!(available, 0);
    assert_eq!(permit_admits(available), Some(true));
    assert_eq!(recovery_admits(available), Some(true));
}

#[test]
fn one_byte_short_is_rejected_by_both_paths() {
    let available = required().unwrap_or_default() - 1;
    assert_eq!(permit_admits(available), Some(false));
    assert_eq!(recovery_admits(available), Some(false));
}

#[test]
fn memory_covering_only_pair_and_reserve_is_rejected_by_both_paths() {
    let retained = retained_tmpfs_bytes().unwrap_or_default();
    assert_ne!(retained, 0);
    let available = required().unwrap_or_default() - retained;
    assert_eq!(permit_admits(available), Some(false));
    assert_eq!(recovery_admits(available), Some(false));
}
