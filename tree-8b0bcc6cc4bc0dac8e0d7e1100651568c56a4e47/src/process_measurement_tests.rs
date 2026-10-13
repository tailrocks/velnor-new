use super::*;

pub(super) fn command(exit: i32) -> Command {
    #[cfg(unix)]
    {
        let mut command = Command::new("sh");
        command.args(["-c", &format!("exit {exit}")]);
        command
    }
    #[cfg(windows)]
    {
        let mut command = Command::new("cmd");
        command.args(["/C", &format!("exit /B {exit}")]);
        command
    }
}

fn process(event: &MeasurementEvent) -> (ProcessOutcome, ProcessMeasurement) {
    match event {
        MeasurementEvent::Process {
            outcome,
            measurement,
            ..
        } => (*outcome, *measurement),
        _ => panic!("expected process event"),
    }
}

#[test]
fn successful_child_and_invocation_emit_once_each() {
    let mut events = Vec::new();
    let mut invocation =
        Invocation::with_sink(AdapterKind::Rustc, InvocationKind::Work, None, |event| {
            events.push(event)
        });
    assert!(
        invocation
            .process(ProcessPurpose::Work)
            .status(&mut command(0))
            .unwrap()
            .success()
    );
    invocation.finish(CacheOutcome::Miss);
    assert_eq!(events.len(), 2);
    let (outcome, measurement) = process(&events[0]);
    assert_eq!(outcome, ProcessOutcome::Succeeded);
    assert_eq!(measurement.attempts, 1);
    assert_eq!(measurement.started, 1);
    assert_eq!(measurement.wall_observations, 1);
    assert!(measurement.observed_wall_ns > 0);
    assert!(matches!(
        events[1],
        MeasurementEvent::Invocation {
            cache_outcome: CacheOutcome::Miss,
            ..
        }
    ));
}

#[test]
fn failed_process_is_independent_of_cache_outcome() {
    let mut events = Vec::new();
    let mut invocation =
        Invocation::with_sink(AdapterKind::Cc, InvocationKind::Work, None, |event| {
            events.push(event)
        });
    assert_eq!(
        invocation
            .process(ProcessPurpose::Work)
            .status(&mut command(17))
            .unwrap()
            .code(),
        Some(17)
    );
    invocation.finish(CacheOutcome::Bypass);
    assert_eq!(process(&events[0]).0, ProcessOutcome::Failed);
    assert!(matches!(
        events[1],
        MeasurementEvent::Invocation {
            cache_outcome: CacheOutcome::Bypass,
            ..
        }
    ));
}

#[test]
fn failed_spawn_has_no_started_child_or_wall_observation() {
    let mut events = Vec::new();
    let temporary = tempfile::tempdir().unwrap();
    let mut missing = Command::new(temporary.path().join("missing-executable"));
    let mut invocation = Invocation::with_sink(
        AdapterKind::BuildScript,
        InvocationKind::Work,
        None,
        |event| events.push(event),
    );
    assert!(
        invocation
            .process(ProcessPurpose::Work)
            .status(&mut missing)
            .is_err()
    );
    assert!(invocation.has_work_attempted());
    invocation.finish(CacheOutcome::Unconsulted);
    assert_eq!(
        process(&events[0]),
        (
            ProcessOutcome::SpawnFailed,
            ProcessMeasurement {
                attempts: 1,
                started: 0,
                observed_wall_ns: 0,
                wall_observations: 0
            }
        )
    );
    assert_eq!(events.len(), 2);
}

#[test]
fn cached_rustdoc_finalization_has_real_child_and_one_hit() {
    let mut events = Vec::new();
    let mut invocation =
        Invocation::with_sink(AdapterKind::Rustdoc, InvocationKind::Work, None, |event| {
            events.push(event)
        });
    invocation
        .process(ProcessPurpose::RustdocFinalize)
        .output(&mut command(0))
        .unwrap();
    invocation.finish(CacheOutcome::Hit);
    assert_eq!(events.len(), 2);
    assert!(matches!(
        events[0],
        MeasurementEvent::Process {
            purpose: ProcessPurpose::RustdocFinalize,
            ..
        }
    ));
    assert!(matches!(
        events[1],
        MeasurementEvent::Invocation {
            cache_outcome: CacheOutcome::Hit,
            ..
        }
    ));
}

#[test]
fn multiple_processes_remain_one_wrapper_invocation() {
    let mut events = Vec::new();
    let mut invocation =
        Invocation::with_sink(AdapterKind::Rustdoc, InvocationKind::Work, None, |event| {
            events.push(event)
        });
    for purpose in [
        ProcessPurpose::Probe,
        ProcessPurpose::Work,
        ProcessPurpose::RustdocFinalize,
    ] {
        invocation.process(purpose).status(&mut command(0)).unwrap();
    }
    invocation.finish(CacheOutcome::Verification);
    assert_eq!(events.len(), 4);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, MeasurementEvent::Invocation { .. }))
            .count(),
        1
    );
}

#[test]
fn unused_process_token_does_not_invent_attempt() {
    let mut events = Vec::new();
    let mut invocation =
        Invocation::with_sink(AdapterKind::Rustc, InvocationKind::Probe, None, |event| {
            events.push(event)
        });
    let _unused = invocation.process(ProcessPurpose::Probe);
    drop(invocation);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0],
        MeasurementEvent::Invocation {
            cache_outcome: CacheOutcome::Unknown,
            ..
        }
    ));
}

#[test]
fn outer_guard_retains_terminal_disposition_and_finishes_once() {
    let mut events = Vec::new();
    let mut invocation =
        Invocation::with_sink(AdapterKind::Cc, InvocationKind::Work, None, |event| {
            events.push(event)
        });
    assert_eq!(invocation.outcome(), CacheOutcome::Unknown);
    invocation.set_outcome(CacheOutcome::Bypass);
    assert_eq!(invocation.outcome(), CacheOutcome::Bypass);
    invocation.finish_current();
    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0],
        MeasurementEvent::Invocation {
            cache_outcome: CacheOutcome::Bypass,
            ..
        }
    ));
}

#[test]
fn dropping_outer_guard_emits_updated_disposition_once() {
    let mut events = Vec::new();
    let mut invocation =
        Invocation::with_sink(AdapterKind::Rustc, InvocationKind::Work, None, |event| {
            events.push(event)
        });
    invocation.set_outcome(CacheOutcome::Miss);
    drop(invocation);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0],
        MeasurementEvent::Invocation {
            cache_outcome: CacheOutcome::Miss,
            ..
        }
    ));
}

#[test]
fn nested_scopes_do_not_replace_each_others_observer() {
    use std::cell::RefCell;
    let events = RefCell::new(Vec::new());
    let mut outer = Invocation::with_sink(
        AdapterKind::BuildScript,
        InvocationKind::Work,
        None,
        |event| events.borrow_mut().push(event),
    );
    {
        let mut nested =
            Invocation::with_sink(AdapterKind::Cc, InvocationKind::Probe, None, |event| {
                events.borrow_mut().push(event)
            });
        nested
            .process(ProcessPurpose::Probe)
            .status(&mut command(0))
            .unwrap();
        nested.finish(CacheOutcome::Bypass);
    }
    outer
        .process(ProcessPurpose::Work)
        .status(&mut command(0))
        .unwrap();
    outer.finish(CacheOutcome::Miss);
    let events = events.into_inner();
    assert_eq!(events.len(), 4);
    assert!(matches!(
        events[1],
        MeasurementEvent::Invocation {
            adapter: AdapterKind::Cc,
            cache_outcome: CacheOutcome::Bypass,
            ..
        }
    ));
    assert!(matches!(
        events[3],
        MeasurementEvent::Invocation {
            adapter: AdapterKind::BuildScript,
            cache_outcome: CacheOutcome::Miss,
            ..
        }
    ));
}

#[test]
fn lost_terminal_observation_remains_unknown_not_zero_sample() {
    let mut events = Vec::new();
    let mut invocation =
        Invocation::with_sink(AdapterKind::Cc, InvocationKind::Work, None, |event| {
            events.push(event)
        });
    let mut measured = invocation
        .process(ProcessPurpose::Work)
        .spawn(&mut command(0))
        .unwrap();
    // Reap through the raw test-owned handle to simulate a caller losing the
    // token's terminal observer without leaving a child behind.
    measured.child.take().unwrap().wait().unwrap();
    drop(measured);
    invocation.finish(CacheOutcome::Bypass);
    assert_eq!(
        process(&events[0]),
        (
            ProcessOutcome::WaitFailed,
            ProcessMeasurement {
                attempts: 1,
                started: 1,
                observed_wall_ns: 0,
                wall_observations: 0
            }
        )
    );
}

#[test]
fn unit_attribution_is_direct_metadata_and_updates_before_spawn() {
    let mut events = Vec::new();
    let unit = UnitIdentity {
        cargo_unit_id: Some("real-unit-id".into()),
        ..UnitIdentity::default()
    };
    let mut invocation =
        Invocation::with_sink(AdapterKind::Rustc, InvocationKind::Work, None, |event| {
            events.push(event)
        });
    invocation.identify(Some(unit.clone()));
    invocation
        .process(ProcessPurpose::Work)
        .status(&mut command(0))
        .unwrap();
    invocation.finish(CacheOutcome::Miss);
    for event in events {
        let observed = match event {
            MeasurementEvent::Invocation { unit, .. }
            | MeasurementEvent::Process { unit, .. }
            | MeasurementEvent::Output { unit, .. } => unit,
        };
        assert_eq!(observed, Some(unit.clone()));
    }
}

#[cfg(unix)]
#[test]
fn signal_termination_is_observed_with_real_wall() {
    let mut events = Vec::new();
    let mut command = Command::new("sh");
    command.args(["-c", "kill -TERM $$"]);
    let mut invocation = Invocation::with_sink(
        AdapterKind::BuildScript,
        InvocationKind::Work,
        None,
        |event| events.push(event),
    );
    assert!(
        !invocation
            .process(ProcessPurpose::Work)
            .status(&mut command)
            .unwrap()
            .success()
    );
    invocation.finish(CacheOutcome::Unconsulted);
    assert_eq!(process(&events[0]).0, ProcessOutcome::Terminated);
    assert_eq!(process(&events[0]).1.wall_observations, 1);
}
