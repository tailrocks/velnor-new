use std::time::{Duration, Instant};

use crate::journal::{StorageFilesystemKind, StorageProfile, fixture_provisioning_profile};

use super::StartPermit;
use super::production_profile::aggregate_guest_capacity;

const ENGINE_ID: &str = "bc9058a0-c807-412b-a088-6c1d96ddd462";

#[test]
fn startup_capacity_uses_all_pairs_and_operational_reserve() -> Result<(), String> {
    use crate::worker::ResourceBudgetConfig;

    let budget = ResourceBudgetConfig {
        runner_cpu_millicores: 2_000,
        runner_memory_bytes: 4 * 1024 * 1024 * 1024,
        dind_cpu_millicores: 2_000,
        dind_memory_bytes: 4 * 1024 * 1024 * 1024,
        operational_memory_reserve_bytes: 1024 * 1024 * 1024,
    }
    .validate()
    .map_err(|error| error.to_string())?;
    let retained = 512 * 1024 * 1024;
    let (cpu, memory) = aggregate_guest_capacity(budget, retained, 2)
        .ok_or_else(|| "The total required capacity is too large.".to_owned())?;
    if budget.pair().cpu_millicores >= 8_000
        || cpu != 9_000
        || memory
            != (budget.pair().memory_bytes + retained) * 2 + budget.admission_reserve().memory_bytes
    {
        return Err(
            "The capacity total does not include both worker pairs and the operational reserve."
                .to_owned(),
        );
    }
    if super::guest_capacity::guest_capacity_fits(8_000, memory, cpu, memory) {
        return Err(
            "The capacity check accepted CPU capacity without the operational reserve.".to_owned(),
        );
    }
    if !super::guest_capacity::guest_capacity_fits(cpu, memory, cpu, memory) {
        return Err(
            "The capacity check rejected the exact required CPU and memory values.".to_owned(),
        );
    }
    if aggregate_guest_capacity(budget, retained, u32::MAX).is_some() {
        return Err(
            "The capacity calculation accepted a job count that makes the total too large."
                .to_owned(),
        );
    }
    let exact = aggregate_guest_capacity(budget, retained, 1).ok_or_else(|| {
        "The total required capacity for one worker pair is too large.".to_owned()
    })?;
    if exact.0 != budget.pair().cpu_millicores + budget.admission_reserve().cpu_millicores {
        return Err("The required CPU total does not include the operational reserve.".to_owned());
    }
    Ok(())
}

fn permit() -> StartPermit {
    let mut permit = StartPermit::test_fixture_with_engine_id(&"a".repeat(32), ENGINE_ID);
    permit.observation.record.memory_available_bytes = 32 * 1024 * 1024 * 1024;
    permit
}

#[test]
fn admission_uses_the_same_fresh_guest_and_filesystem_sample() -> Result<(), String> {
    let sample_permit = permit();
    let root = sample_permit.docker_root_digest().to_owned();
    let profile = fixture_provisioning_profile();
    let admission = sample_permit
        .storage_provisioning_admission(&profile, ENGINE_ID, &root)
        .ok_or_else(|| "matching fresh sample was rejected".to_owned())?;
    let sample = admission.sample();
    if sample.identity().sample_generation_id() != sample_permit.sample_generation_id()
        || sample.identity().profile_generation_id() != profile.generation_id()
        || sample.identity().profile_digest() != profile.profile_digest()
        || sample.identity().engine_id() != ENGINE_ID
        || sample.identity().docker_root_digest() != root
        || sample.pools().len() != 1
        || sample.pools()[0].filesystem_kind() != StorageFilesystemKind::Btrfs
        || sample.pools()[0].total_inodes() != 0
        || sample.pools()[0].available_inodes() != 0
        || admission.memory().guest_total_bytes() != 256 * 1024 * 1024 * 1024
    {
        return Err("sample identity or selected-guest facts changed".to_owned());
    }
    admission
        .validate_for(
            &profile,
            sample_permit.sample_generation_id(),
            ENGINE_ID,
            &root,
            sample_permit.charge().1,
            Instant::now(),
        )
        .map_err(|error| error.to_string())
}

#[test]
fn admission_rejects_wrong_engine_root_and_stale_samples() {
    let profile = fixture_provisioning_profile();
    let sample_permit = permit();
    let root = sample_permit.docker_root_digest().to_owned();
    assert!(
        sample_permit
            .storage_provisioning_admission(&profile, "other-engine", &root)
            .is_none()
    );
    assert!(
        sample_permit
            .storage_provisioning_admission(&profile, ENGINE_ID, &"b".repeat(64))
            .is_none()
    );

    let mut stale = permit();
    stale.observation.observed_at = Instant::now() - Duration::from_secs(31);
    assert!(
        stale
            .storage_provisioning_admission(&profile, ENGINE_ID, &root)
            .is_none()
    );
}

#[test]
fn filesystem_identity_does_not_depend_on_namespace_local_mount_id() -> Result<(), String> {
    let profile = fixture_provisioning_profile();
    let first = permit();
    let mut second = permit();
    second.observation.record.docker_root_mount_id += 100;
    let root = first.docker_root_digest().to_owned();
    let first = first
        .storage_provisioning_admission(&profile, ENGINE_ID, &root)
        .ok_or_else(|| "first sample was rejected".to_owned())?;
    let second = second
        .storage_provisioning_admission(&profile, ENGINE_ID, &root)
        .ok_or_else(|| "second sample was rejected".to_owned())?;
    if first.sample().pools()[0].filesystem_instance_digest()
        != second.sample().pools()[0].filesystem_instance_digest()
    {
        return Err("namespace-local mount ID changed filesystem identity".to_owned());
    }
    Ok(())
}

#[test]
fn alternate_profile_generation_fails_journal_validation() -> Result<(), String> {
    let permit = permit();
    let root = permit.docker_root_digest().to_owned();
    let current = fixture_provisioning_profile();
    let other = StorageProfile::new(
        "c".repeat(32),
        current.pools().to_vec(),
        current.roles().to_vec(),
    )
    .map_err(|error| error.to_string())?;
    let admission = permit
        .storage_provisioning_admission(&other, ENGINE_ID, &root)
        .ok_or_else(|| "valid alternate profile was not sampleable".to_owned())?;
    assert!(
        admission
            .validate_for(
                &current,
                permit.sample_generation_id(),
                ENGINE_ID,
                &root,
                permit.charge().1,
                Instant::now(),
            )
            .is_err()
    );
    Ok(())
}
