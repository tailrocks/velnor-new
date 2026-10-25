use crate::journal::StorageMedium;
use crate::launch::resource_probe::StartPermit;

use super::{admission_witness_is_fresh, capacity::usage_credit};

#[test]
fn exclusive_tmpfs_credit_uses_checked_used_totals() {
    assert_eq!(
        usage_credit(StorageMedium::Tmpfs, 4096, 3072, 128, 96),
        Ok((1024, 32))
    );
}

#[test]
fn shared_disk_receives_no_capacity_credit() {
    assert_eq!(
        usage_credit(StorageMedium::Disk, 64 * 1024, 60 * 1024, 4096, 3900),
        Ok((0, 0))
    );
}

#[test]
fn impossible_free_counts_fail_closed() {
    assert!(usage_credit(StorageMedium::Tmpfs, 10, 11, 10, 10).is_err());
    assert!(usage_credit(StorageMedium::Tmpfs, 10, 9, 10, 11).is_err());
}

#[test]
fn recovered_observation_keeps_its_admission_binding() {
    const ENGINE: &str = "test-engine";
    const ROOT: &str = "8e6e63369fac77108c139089d93aab24c21d5e048d58c66cee440febed124ab2";
    let observation_sample = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let admission_sample = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let unrelated_sample = "cccccccccccccccccccccccccccccccc";

    let recovery = StartPermit::test_fixture_with_available_memory(
        observation_sample,
        16 * 1024 * 1024 * 1024,
    )
    .into_test_recovery_observation()
    .expect("The fixture permits recovery.");
    assert!(
        recovery
            .witness()
            .is_fresh_for(ENGINE, ROOT, observation_sample)
    );
    let charge = recovery.charge();
    let (recovered, _) = recovery
        .consume_for_recovery(ENGINE, ROOT, charge, admission_sample)
        .expect("The recovery sample uses the admission ID.");

    assert!(recovered.is_fresh_for(ENGINE, ROOT, observation_sample));
    assert!(admission_witness_is_fresh(
        &recovered,
        ENGINE,
        ROOT,
        admission_sample
    ));
    assert!(!admission_witness_is_fresh(
        &recovered,
        ENGINE,
        ROOT,
        observation_sample
    ));
    assert!(!admission_witness_is_fresh(
        &recovered,
        ENGINE,
        ROOT,
        unrelated_sample
    ));
}

#[test]
fn fresh_observation_cannot_cross_admission_sample() {
    const ENGINE: &str = "test-engine";
    const ROOT: &str = "8e6e63369fac77108c139089d93aab24c21d5e048d58c66cee440febed124ab2";
    let different_sample = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let (fresh, observation_sample) =
        crate::launch::StartObservationWitness::test_fixture_for(ENGINE, ROOT, false);

    assert!(admission_witness_is_fresh(
        &fresh,
        ENGINE,
        ROOT,
        &observation_sample
    ));
    assert!(!admission_witness_is_fresh(
        &fresh,
        ENGINE,
        ROOT,
        different_sample
    ));
}
