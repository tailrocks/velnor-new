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
