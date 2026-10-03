use super::*;
use crate::{CacheOutcome, InvocationKind, ProcessPurpose};

fn invocation(unit: Option<UnitIdentity>) -> MeasurementEvent {
    MeasurementEvent::Invocation {
        adapter: AdapterKind::Rustc,
        invocation_kind: InvocationKind::Work,
        cache_outcome: CacheOutcome::Miss,
        unit,
    }
}
fn process(outcome: ProcessOutcome, measurement: ProcessMeasurement) -> MeasurementEvent {
    MeasurementEvent::Process {
        adapter: AdapterKind::Rustdoc,
        purpose: ProcessPurpose::RustdocFinalize,
        outcome,
        measurement,
        unit: None,
    }
}
#[test]
fn invocation_totals_and_unknown_unit_are_retained() {
    let destination = Measurements::default();
    record(&destination, invocation(None)).unwrap();
    record(&destination, invocation(None)).unwrap();
    let stats = snapshot(&destination);
    let adapter = &stats[&AdapterKind::Rustc];
    assert_eq!(
        adapter.invocations[&InvocationKind::Work][&CacheOutcome::Miss],
        2
    );
    assert_eq!(adapter.units.len(), 1);
    assert!(adapter.units[0].identity.is_none());
    assert_eq!(adapter.units[0].invocations, adapter.invocations);
}
#[test]
fn spawn_and_wait_failures_are_not_zero_wall_observations() {
    let destination = Measurements::default();
    for (outcome, started) in [
        (ProcessOutcome::SpawnFailed, 0),
        (ProcessOutcome::WaitFailed, 1),
    ] {
        record(
            &destination,
            process(
                outcome,
                ProcessMeasurement {
                    attempts: 1,
                    started,
                    observed_wall_ns: 0,
                    wall_observations: 0,
                },
            ),
        )
        .unwrap();
    }
    let stats = snapshot(&destination);
    for measured in
        stats[&AdapterKind::Rustdoc].subprocesses[&ProcessPurpose::RustdocFinalize].values()
    {
        assert_eq!(measured.attempts, 1);
        assert_eq!(measured.wall_observations, 0);
    }
}
#[test]
fn successful_cached_rustdoc_finalize_remains_real_work() {
    let destination = Measurements::default();
    record(
        &destination,
        MeasurementEvent::Invocation {
            adapter: AdapterKind::Rustdoc,
            invocation_kind: InvocationKind::Work,
            cache_outcome: CacheOutcome::Hit,
            unit: None,
        },
    )
    .unwrap();
    record(
        &destination,
        process(
            ProcessOutcome::Succeeded,
            ProcessMeasurement {
                attempts: 1,
                started: 1,
                observed_wall_ns: 42,
                wall_observations: 1,
            },
        ),
    )
    .unwrap();
    let stats = snapshot(&destination);
    let adapter = &stats[&AdapterKind::Rustdoc];
    assert_eq!(
        adapter.invocations[&InvocationKind::Work][&CacheOutcome::Hit],
        1
    );
    assert_eq!(
        adapter.subprocesses[&ProcessPurpose::RustdocFinalize][&ProcessOutcome::Succeeded]
            .observed_wall_ns,
        42
    );
}
#[test]
fn inconsistent_attempt_is_rejected_before_any_aggregate_changes() {
    let destination = Measurements::default();
    assert!(
        record(
            &destination,
            process(
                ProcessOutcome::SpawnFailed,
                ProcessMeasurement {
                    attempts: 1,
                    started: 0,
                    observed_wall_ns: 0,
                    wall_observations: 1,
                }
            )
        )
        .is_err()
    );
    assert!(snapshot(&destination).is_empty());
}
#[test]
fn unit_bound_retains_totals_existing_units_and_unknown_bucket() {
    let destination = Measurements::default();
    for index in 0..MAX_UNITS_PER_ADAPTER + 1 {
        record(
            &destination,
            invocation(Some(UnitIdentity {
                cargo_unit_id: Some(index.to_string()),
                ..Default::default()
            })),
        )
        .unwrap();
    }
    record(&destination, invocation(None)).unwrap();
    record(
        &destination,
        invocation(Some(UnitIdentity {
            cargo_unit_id: Some("0".into()),
            ..Default::default()
        })),
    )
    .unwrap();
    let stats = snapshot(&destination);
    let adapter = &stats[&AdapterKind::Rustc];
    assert_eq!(adapter.units.len(), MAX_UNITS_PER_ADAPTER + 1);
    assert_eq!(adapter.omitted_unit_events, 1);
    assert_eq!(
        adapter.invocations[&InvocationKind::Work][&CacheOutcome::Miss],
        (MAX_UNITS_PER_ADAPTER + 3) as u64
    );
    assert!(adapter.units[0].identity.is_none());
    let known = adapter
        .units
        .iter()
        .find(|unit| {
            unit.identity
                .as_ref()
                .is_some_and(|identity| identity.cargo_unit_id.as_deref() == Some("0"))
        })
        .unwrap();
    assert_eq!(
        known.invocations[&InvocationKind::Work][&CacheOutcome::Miss],
        2
    );
}
#[test]
fn missing_package_identity_cannot_assert_workspace_ownership() {
    let destination = Measurements::default();
    assert!(
        record(
            &destination,
            invocation(Some(UnitIdentity {
                origin: PackageOrigin::Workspace,
                ..Default::default()
            }))
        )
        .is_err()
    );
    assert!(snapshot(&destination).is_empty());
}

fn output() -> crate::OutputObservation {
    crate::OutputObservation {
        path: std::env::temp_dir().join("measurement-output.rlib"),
        aliases: vec![],
        cache_outcome: CacheOutcome::Hit,
        digest: crate::CacheDigest::blake3(b"actual test bytes"),
        file_identity: None,
    }
}
fn output_event(observation: crate::OutputObservation) -> MeasurementEvent {
    MeasurementEvent::Output {
        adapter: AdapterKind::Rustc,
        unit: None,
        observation,
    }
}
#[test]
fn output_evidence_never_increments_invocation_or_process_counts() {
    let destination = Measurements::default();
    record(&destination, invocation(None)).unwrap();
    record(&destination, output_event(output())).unwrap();
    let stats = snapshot(&destination);
    let adapter = &stats[&AdapterKind::Rustc];
    assert_eq!(
        adapter.invocations[&InvocationKind::Work][&CacheOutcome::Miss],
        1
    );
    assert!(adapter.subprocesses.is_empty());
    assert_eq!(adapter.units[0].outputs, vec![output()]);
}
#[test]
fn invalid_digest_or_duplicate_alias_is_rejected_before_accumulation() {
    let destination = Measurements::default();
    let mut invalid = output();
    invalid.digest.hash = "invalid".into();
    assert!(record(&destination, output_event(invalid)).is_err());
    let mut duplicate = output();
    duplicate.aliases.push(duplicate.path.clone());
    assert!(record(&destination, output_event(duplicate)).is_err());
    assert!(snapshot(&destination).is_empty());
}
#[test]
fn output_bound_is_explicit_and_does_not_erase_actual_counts() {
    let destination = Measurements::default();
    record(&destination, invocation(None)).unwrap();
    destination
        .lock()
        .unwrap()
        .get_mut(&AdapterKind::Rustc)
        .unwrap()
        .units[0]
        .outputs = vec![output(); MAX_OUTPUTS_PER_ADAPTER];
    record(&destination, output_event(output())).unwrap();
    let stats = snapshot(&destination);
    let adapter = &stats[&AdapterKind::Rustc];
    assert_eq!(adapter.units[0].outputs.len(), MAX_OUTPUTS_PER_ADAPTER);
    assert_eq!(adapter.omitted_output_events, 1);
    assert_eq!(adapter.units[0].omitted_output_events, 1);
    assert_eq!(
        adapter.invocations[&InvocationKind::Work][&CacheOutcome::Miss],
        1
    );
}

fn retained_output() -> crate::OutputObservation {
    let mut observation = output();
    observation.file_identity = Some(crate::FileIdentity {
        path: observation.path.clone(),
        len: observation.digest.size,
        modified: std::time::SystemTime::UNIX_EPOCH,
        changed: None,
        object: Some(crate::FileObjectIdentity {
            device_major: 1,
            device_minor: 2,
            mount_id: 3,
            inode: 4,
        }),
    });
    observation
}
#[test]
fn retained_output_identity_must_match_original_path_and_size() {
    let destination = Measurements::default();
    let mut wrong_path = retained_output();
    wrong_path.file_identity.as_mut().unwrap().path =
        std::env::temp_dir().join("other-output.rlib");
    assert!(record(&destination, output_event(wrong_path)).is_err());
    let mut wrong_size = retained_output();
    wrong_size.file_identity.as_mut().unwrap().len += 1;
    assert!(record(&destination, output_event(wrong_size)).is_err());
    assert!(snapshot(&destination).is_empty());
    record(&destination, output_event(retained_output())).unwrap();
    let observed = snapshot(&destination);
    assert_eq!(
        observed[&AdapterKind::Rustc].units[0].outputs,
        vec![retained_output()]
    );
}
#[test]
fn aliases_without_retained_physical_identity_stay_unavailable() {
    let destination = Measurements::default();
    let mut unsupported = output();
    unsupported
        .aliases
        .push(std::env::temp_dir().join("same-content-copy.rlib"));
    assert!(record(&destination, output_event(unsupported)).is_err());
    assert!(snapshot(&destination).is_empty());
}
