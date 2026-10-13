use std::time::{Duration, Instant};

use super::super::consume_permit;
use super::super::record::ProbeRecord;
use super::super::sample::Observation;
use crate::worker::test_resource_budget;

#[test]
fn sample_permit_requires_recent_bounded_metrics_and_matching_engine() -> Result<(), String> {
    let budget = test_resource_budget().map_err(|error| error.to_string())?;
    let pair = budget.pair();
    let mem_total = pair.memory_bytes + 1;
    let root_digest = "a".repeat(64);
    let sample_record = ProbeRecord {
        schema_version: 1,
        docker_root_free_bytes: 20 * 1024 * 1024 * 1024,
        docker_root_total_bytes: 40 * 1024 * 1024 * 1024,
        memory_available_bytes: pair.memory_bytes,
        load_milli: 125,
        memory_psi_some_avg10_bps: None,
    };
    let sample = Observation::new(
        sample_record.clone(),
        8,
        mem_total,
        "selected-engine".to_owned(),
        root_digest.clone(),
        Instant::now(),
    )
    .ok_or_else(|| "valid guest record was rejected".to_owned())?;
    let permit = sample
        .into_start_permit(budget)
        .ok_or_else(|| "valid fresh sample did not yield a permit".to_owned())?;
    assert!(consume_permit(permit, "selected-engine", &root_digest));

    let stale_at = Instant::now()
        .checked_sub(Duration::from_millis(30_500))
        .ok_or_else(|| "could not construct stale instant".to_owned())?;
    let stale = Observation::new(
        sample_record.clone(),
        8,
        mem_total,
        "selected-engine".to_owned(),
        root_digest.clone(),
        stale_at,
    )
    .ok_or_else(|| "stale sample record was rejected too early".to_owned())?;
    assert!(stale.into_start_permit(budget).is_none());

    let created_at = Instant::now();
    let fresh = Observation::new(
        sample_record.clone(),
        8,
        mem_total,
        "test-engine".to_owned(),
        root_digest.clone(),
        created_at,
    )
    .ok_or_else(|| "valid sample record was rejected".to_owned())?;
    let permit = fresh
        .into_start_permit(budget)
        .ok_or_else(|| "fresh sample did not yield a permit".to_owned())?;
    let expires_at = created_at
        .checked_add(Duration::from_millis(30_500))
        .ok_or_else(|| "could not construct permit expiry instant".to_owned())?;
    assert!(!permit.test_is_valid_at(expires_at));

    let mut low_disk = sample_record.clone();
    low_disk.docker_root_free_bytes = 10 * 1024 * 1024 * 1024 - 1;
    let low_disk = Observation::new(
        low_disk,
        8,
        mem_total,
        "selected-engine".to_owned(),
        root_digest.clone(),
        Instant::now(),
    )
    .ok_or_else(|| "low disk record was rejected before admission".to_owned())?;
    assert!(low_disk.into_start_permit(budget).is_none());

    let wrong_engine = Observation::new(
        sample_record,
        8,
        mem_total,
        "selected-engine".to_owned(),
        root_digest.clone(),
        Instant::now(),
    )
    .ok_or_else(|| "valid sample record was rejected".to_owned())?
    .into_start_permit(budget)
    .ok_or_else(|| "valid fresh sample did not yield a permit".to_owned())?;
    assert!(!consume_permit(wrong_engine, "other-engine", &root_digest));
    Ok(())
}
