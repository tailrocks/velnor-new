use std::time::{Duration, Instant};

use super::super::consume_permit;
use super::super::record::ProbeRecord;
use super::super::sample::{Observation, ObservationTestInput};
use crate::worker::{ResourceBudget, test_resource_budget};

fn sample_record(disk_free_bytes: u64) -> ProbeRecord {
    let mut record = super::super::record::fixture_record();
    record.docker_root_free_bytes = disk_free_bytes;
    record.docker_root_available_bytes = disk_free_bytes;
    record.memory_available_bytes = 10 * 1024 * 1024 * 1024;
    record
}

#[test]
fn doctor_report_uses_selected_guest_values_and_expires_after_thirty_seconds() -> Result<(), String>
{
    let budget = test_resource_budget().map_err(|error| error.to_string())?;
    let root_digest = "a".repeat(64);
    let recent = observation(
        sample_record(24 * 1024 * 1024 * 1024),
        budget,
        "selected-engine",
        &root_digest,
        Instant::now(),
    )?
    .doctor_report()
    .ok_or_else(|| "fresh guest metrics were rejected".to_owned())?;
    assert_eq!(recent["load_milli"], 125);
    assert_eq!(recent["cpu_count"], 8);
    assert_eq!(
        recent["docker_root_total_bytes"],
        40 * 1024 * 1024 * 1024_u64
    );
    assert_eq!(
        recent["docker_root_available_bytes"],
        24 * 1024 * 1024 * 1024_u64
    );
    assert_eq!(recent["docker_root_inode_total"], 0);
    assert_eq!(recent["docker_root_inode_free"], 0);
    assert_eq!(recent["docker_root_inode_available"], 0);
    assert_eq!(recent["docker_root_inode_policy"], "dynamic_btrfs");
    assert_eq!(recent["docker_root_fs_type_name"], "btrfs");
    assert_eq!(recent["docker_root_filesystem_id"], 17);
    assert!(recent["memory_psi_some_avg10_bps"].is_null());
    assert_eq!(
        recent["memory_available_bytes"],
        10 * 1024 * 1024 * 1024_u64
    );
    assert_eq!(
        recent["docker_root_free_bytes"],
        24 * 1024 * 1024 * 1024_u64
    );

    let stale_at = Instant::now()
        .checked_sub(Duration::from_millis(30_500))
        .ok_or_else(|| "could not construct stale instant".to_owned())?;
    let stale = observation(
        sample_record(24 * 1024 * 1024 * 1024),
        budget,
        "selected-engine",
        &root_digest,
        stale_at,
    )?;
    assert!(stale.doctor_report().is_none());
    Ok(())
}

#[test]
fn observation_rejects_startup_evidence_for_another_engine_or_root() -> Result<(), String> {
    let engine_id = "selected-engine";
    let root_digest = "a".repeat(64);
    if observation_with_startup_evidence(engine_id, &root_digest, engine_id, &root_digest).is_none()
    {
        return Err("matching probe startup evidence was rejected".to_owned());
    }
    if observation_with_startup_evidence(engine_id, &root_digest, "other-engine", &root_digest)
        .is_some()
    {
        return Err("probe startup evidence from another engine was accepted".to_owned());
    }
    if observation_with_startup_evidence(engine_id, &root_digest, engine_id, &"b".repeat(64))
        .is_some()
    {
        return Err("probe startup evidence from another root was accepted".to_owned());
    }
    Ok(())
}

fn observation_with_startup_evidence(
    engine_id: &str,
    root_digest: &str,
    evidence_engine_id: &str,
    evidence_root_digest: &str,
) -> Option<Observation> {
    let record = sample_record(24 * 1024 * 1024 * 1024);
    let generation = "f".repeat(32);
    let startup_evidence = super::super::ProbeStartupEvidence::test_fixture(
        &generation,
        evidence_engine_id,
        evidence_root_digest,
    );
    let memory_total_bytes = record.docker_root_total_bytes;
    let inspected_host_pid = record.probe_host_pid;
    Observation::new(super::super::sample::ObservationInput {
        record,
        cpus: 8,
        inspected_host_pid,
        memory_total_bytes,
        engine_id: engine_id.to_owned(),
        docker_root_digest: root_digest.to_owned(),
        sample_generation_id: generation,
        observed_at: Instant::now(),
        startup_evidence,
    })
}

fn observation(
    record: ProbeRecord,
    budget: ResourceBudget,
    engine_id: &str,
    root_digest: &str,
    observed_at: Instant,
) -> Result<Observation, String> {
    observation_with_cpu(record, 8, budget, engine_id, root_digest, observed_at)
}

fn observation_with_cpu(
    record: ProbeRecord,
    cpus: u32,
    budget: ResourceBudget,
    engine_id: &str,
    root_digest: &str,
    observed_at: Instant,
) -> Result<Observation, String> {
    let memory_total_bytes = record
        .docker_root_total_bytes
        .max(budget.pair().memory_bytes);
    let inspected_host_pid = record.probe_host_pid;
    Observation::test_new(ObservationTestInput {
        record,
        cpus,
        inspected_host_pid,
        memory_total_bytes,
        engine_id: engine_id.to_owned(),
        docker_root_digest: root_digest.to_owned(),
        sample_generation_id: "f".repeat(32),
        observed_at,
    })
    .ok_or_else(|| "valid guest record was rejected".to_owned())
}

#[test]
fn sample_permit_requires_matching_engine_and_root() -> Result<(), String> {
    let budget = test_resource_budget().map_err(|error| error.to_string())?;
    let root_digest = "a".repeat(64);
    let permit = observation(
        sample_record(20 * 1024 * 1024 * 1024),
        budget,
        "selected-engine",
        &root_digest,
        Instant::now(),
    )?
    .into_start_permit(budget)
    .ok_or_else(|| "fresh valid sample did not yield a permit".to_owned())?;
    assert!(consume_permit(permit, "selected-engine", &root_digest));

    let wrong_engine = observation(
        sample_record(20 * 1024 * 1024 * 1024),
        budget,
        "selected-engine",
        &root_digest,
        Instant::now(),
    )?
    .into_start_permit(budget)
    .ok_or_else(|| "fresh valid sample did not yield a permit".to_owned())?;
    assert!(!consume_permit(wrong_engine, "other-engine", &root_digest));

    let wrong_root = observation(
        sample_record(20 * 1024 * 1024 * 1024),
        budget,
        "selected-engine",
        &root_digest,
        Instant::now(),
    )?
    .into_start_permit(budget)
    .ok_or_else(|| "fresh valid sample did not yield a permit".to_owned())?;
    assert!(!consume_permit(
        wrong_root,
        "selected-engine",
        &"b".repeat(64)
    ));
    Ok(())
}

#[test]
fn sample_permit_rejects_stale_expired_and_low_disk_samples() -> Result<(), String> {
    let budget = test_resource_budget().map_err(|error| error.to_string())?;
    let root_digest = "a".repeat(64);
    let stale_at = Instant::now()
        .checked_sub(Duration::from_millis(30_500))
        .ok_or_else(|| "could not construct stale instant".to_owned())?;
    let stale = observation(
        sample_record(20 * 1024 * 1024 * 1024),
        budget,
        "selected-engine",
        &root_digest,
        stale_at,
    )?;
    assert!(stale.into_start_permit(budget).is_none());

    let created_at = Instant::now();
    let fresh = observation(
        sample_record(20 * 1024 * 1024 * 1024),
        budget,
        "test-engine",
        &root_digest,
        created_at,
    )?
    .into_start_permit(budget)
    .ok_or_else(|| "fresh valid sample did not yield a permit".to_owned())?;
    let expires_at = created_at
        .checked_add(Duration::from_millis(30_500))
        .ok_or_else(|| "could not construct permit expiry instant".to_owned())?;
    assert!(!fresh.test_is_valid_at(expires_at));

    let low_disk = observation(
        sample_record(10 * 1024 * 1024 * 1024 - 1),
        budget,
        "selected-engine",
        &root_digest,
        Instant::now(),
    )?;
    assert!(low_disk.into_start_permit(budget).is_none());
    Ok(())
}

#[test]
fn sample_permit_requires_pair_plus_reserves_at_exact_cpu_memory_and_disk_bounds()
-> Result<(), String> {
    let budget = test_resource_budget().map_err(|error| error.to_string())?;
    let reserve = budget.admission_reserve();
    let pair = budget.pair();
    let required_memory = pair
        .memory_bytes
        .checked_add(reserve.memory_bytes)
        .ok_or_else(|| "memory requirement overflow".to_owned())?;
    let required_cpu = pair
        .cpu_millicores
        .checked_add(reserve.cpu_millicores)
        .ok_or_else(|| "CPU requirement overflow".to_owned())?;
    let required_cpus = u32::try_from(required_cpu / 1_000)
        .map_err(|_| "CPU requirement out of range".to_owned())?;
    let root = "a".repeat(64);

    let mut exact_record = sample_record(10 * 1024 * 1024 * 1024);
    exact_record.memory_available_bytes = required_memory;
    let exact = observation_with_cpu(
        exact_record,
        required_cpus,
        budget,
        "selected-engine",
        &root,
        Instant::now(),
    )?
    .into_start_permit(budget);
    assert!(exact.is_some());

    let mut low_memory_record = sample_record(10 * 1024 * 1024 * 1024);
    low_memory_record.memory_available_bytes = required_memory - 1;
    assert!(
        observation_with_cpu(
            low_memory_record,
            required_cpus,
            budget,
            "selected-engine",
            &root,
            Instant::now(),
        )?
        .into_start_permit(budget)
        .is_none()
    );

    let mut low_cpu_record = sample_record(10 * 1024 * 1024 * 1024);
    low_cpu_record.memory_available_bytes = required_memory;
    assert!(
        observation_with_cpu(
            low_cpu_record,
            required_cpus - 1,
            budget,
            "selected-engine",
            &root,
            Instant::now(),
        )?
        .into_start_permit(budget)
        .is_none()
    );

    let exactly_ten_gib_disk = observation_with_cpu(
        sample_record(10 * 1024 * 1024 * 1024),
        required_cpus,
        budget,
        "selected-engine",
        &root,
        Instant::now(),
    )?
    .into_start_permit(budget);
    assert!(exactly_ten_gib_disk.is_some());
    Ok(())
}
