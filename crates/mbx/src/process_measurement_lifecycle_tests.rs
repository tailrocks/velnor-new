use super::tests::command;
use super::*;

#[test]
fn explicit_flush_then_finish_and_drop_emit_once() {
    let mut events = Vec::new();
    let mut invocation =
        Invocation::with_sink(AdapterKind::Cc, InvocationKind::Work, None, |event| {
            events.push(event)
        });
    invocation.set_outcome(CacheOutcome::Hit);
    invocation.finish_now();
    invocation.finish_now();
    invocation.finish_current();
    assert_eq!(events.len(), 1);
}

#[test]
fn successful_probe_does_not_suppress_workload_fallback() {
    let mut events = Vec::new();
    let mut invocation =
        Invocation::with_sink(AdapterKind::Rustc, InvocationKind::Work, None, |event| {
            events.push(event)
        });
    invocation
        .process(ProcessPurpose::Probe)
        .status(&mut command(0))
        .unwrap();
    assert!(!invocation.has_work_attempted());
    invocation
        .process(ProcessPurpose::Work)
        .status(&mut command(0))
        .unwrap();
    assert!(invocation.has_work_attempted());
    invocation.finish(CacheOutcome::Bypass);
}

#[test]
fn next_failed_work_attempt_is_distinct_from_prior_success() {
    let mut events = Vec::new();
    let mut invocation =
        Invocation::with_sink(AdapterKind::Rustdoc, InvocationKind::Work, None, |event| {
            events.push(event)
        });
    let status = invocation
        .process(ProcessPurpose::Work)
        .status(&mut command(0))
        .unwrap();
    assert!(status.success());
    let directory = tempfile::tempdir().unwrap();
    let mut missing = Command::new(directory.path().join("missing"));
    assert!(
        invocation
            .process(ProcessPurpose::RustdocFinalize)
            .status(&mut missing)
            .is_err()
    );
    assert!(invocation.has_work_attempted());
    invocation.finish_current();
    assert!(matches!(
        events[0],
        MeasurementEvent::Process {
            outcome: ProcessOutcome::Succeeded,
            ..
        }
    ));
    assert!(matches!(
        events[1],
        MeasurementEvent::Process {
            outcome: ProcessOutcome::SpawnFailed,
            ..
        }
    ));
}

#[test]
fn probe_after_failed_work_remains_a_separate_observation() {
    let mut events = Vec::new();
    let mut invocation =
        Invocation::with_sink(AdapterKind::Rustc, InvocationKind::Work, None, |event| {
            events.push(event)
        });
    let status = invocation
        .process(ProcessPurpose::Work)
        .status(&mut command(13))
        .unwrap();
    invocation
        .process(ProcessPurpose::Probe)
        .status(&mut command(0))
        .unwrap();
    assert_eq!(status.code(), Some(13));
    invocation.finish_current();
    assert!(matches!(
        events[0],
        MeasurementEvent::Process {
            purpose: ProcessPurpose::Work,
            outcome: ProcessOutcome::Failed,
            ..
        }
    ));
    assert!(matches!(
        events[1],
        MeasurementEvent::Process {
            purpose: ProcessPurpose::Probe,
            outcome: ProcessOutcome::Succeeded,
            ..
        }
    ));
}

#[test]
#[cfg(unix)]
fn native_lost_ack_preserves_success_and_leaves_failure_evidence() {
    let temporary = tempfile::tempdir().unwrap();
    let reports = temporary.path().canonicalize().unwrap();
    let output = isolated_native_fixture(&reports, Some(&reports.join("nonexistent.sock")));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let files: Vec<_> = std::fs::read_dir(reports)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        files
            .iter()
            .filter(|name| name.ends_with("measurement-attempt.json"))
            .count(),
        2
    );
    assert_eq!(
        files
            .iter()
            .filter(|name| name.ends_with("measurement-failure.json"))
            .count(),
        2
    );
    assert!(
        !files
            .iter()
            .any(|name| name.ends_with("measurement-ack.json"))
    );
}

#[test]
fn native_lost_ack_fixture() {
    if std::env::var_os("MBX_NATIVE_LOST_ACK_TEST").is_none() {
        return;
    }
    let mut invocation = Invocation::new(AdapterKind::Cc, InvocationKind::Work, None);
    let status = invocation
        .process(ProcessPurpose::Work)
        .status(&mut command(0))
        .unwrap();
    assert!(status.success());
    invocation.set_outcome(CacheOutcome::Bypass);
    invocation.finish_current();
}

#[test]
fn configured_scope_without_socket_is_explicitly_unavailable() {
    let temporary = tempfile::tempdir().unwrap();
    let reports = temporary.path().canonicalize().unwrap();
    let output = isolated_native_fixture(&reports, None);
    assert!(output.status.success());
    assert_eq!(std::fs::read_dir(&reports).unwrap().count(), 0);
    let diagnostics = String::from_utf8(output.stderr).unwrap();
    assert!(diagnostics.contains("MBX_MEASUREMENT_UNAVAILABLE"));
    assert!(diagnostics.contains("session_unavailable"));
    assert!(diagnostics.contains("enrollment_missing"));
}

#[test]
fn failed_enrollment_preserves_work_and_cannot_emit_ack() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    let reports = root.join("not-a-directory");
    std::fs::write(&reports, b"original").unwrap();
    let output = isolated_native_fixture(&reports, Some(&root.join("missing.sock")));
    assert!(output.status.success());
    assert_eq!(std::fs::read(&reports).unwrap(), b"original");
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    let diagnostics = String::from_utf8(output.stderr).unwrap();
    assert!(diagnostics.contains("enrollment_write_failed"));
    assert!(diagnostics.contains("enrollment_missing"));
    assert!(!diagnostics.contains(&reports.to_string_lossy().to_string()));
}

fn isolated_native_fixture(reports: &std::path::Path, socket: Option<&std::path::Path>) -> Output {
    use crate::session::completed_report::{CommandRole, SessionIdentity};
    let identity = SessionIdentity::new(CommandRole::Exec, None, None, None).unwrap();
    let _admission = if cfg!(all(unix, feature = "owned-cache-transport"))
        && socket.is_some()
        && reports.is_dir()
    {
        Some(crate::dispatch_admission::AdmissionOwner::open(reports, &identity).unwrap())
    } else {
        None
    };
    let mut fixture = Command::new(std::env::current_exe().unwrap());
    fixture
        .args([
            "--exact",
            "process_measurement::lifecycle_tests::native_lost_ack_fixture",
            "--nocapture",
        ])
        .env("MBX_NATIVE_LOST_ACK_TEST", "1")
        .env("MBX_STATS_REPORT_DIR", reports)
        .env("MBX_REPORT_SESSION_ID", &identity.session_id)
        .env("MBX_REPORT_ROOT_SESSION_ID", &identity.root_session_id)
        .env_remove("MBX_REPORT_CORRELATION_ID")
        .env_remove("MBX_REPORT_PARENT_SESSION_ID");
    if let Some(socket) = socket {
        fixture.env("MBX_SOCKET", socket);
    } else {
        fixture.env_remove("MBX_SOCKET");
    }
    fixture.output().unwrap()
}

#[test]
fn output_observation_uses_owning_unit_and_current_disposition() {
    let mut events = Vec::new();
    let unit = UnitIdentity {
        cargo_unit_id: Some("actual-unit".into()),
        ..UnitIdentity::default()
    };
    let mut invocation = Invocation::with_sink(
        AdapterKind::Rustc,
        InvocationKind::Work,
        Some(unit.clone()),
        |event| events.push(event),
    );
    invocation.set_outcome(CacheOutcome::Hit);
    invocation.record_output(OutputObservation {
        path: "/native-output".into(),
        aliases: Vec::new(),
        cache_outcome: CacheOutcome::Miss,
        digest: mbx_cache_core::CacheDigest::blake3(b"native bytes"),
        file_identity: None,
    });
    invocation.finish_current();
    assert_eq!(events.len(), 2);
    match &events[0] {
        MeasurementEvent::Output {
            adapter,
            unit: observed_unit,
            observation,
        } => {
            assert_eq!(*adapter, AdapterKind::Rustc);
            assert_eq!(observed_unit.as_ref(), Some(&unit));
            assert_eq!(observation.cache_outcome, CacheOutcome::Hit);
        }
        other => panic!("expected output observation, got {other:?}"),
    }
    assert!(matches!(
        events[1],
        MeasurementEvent::Invocation {
            cache_outcome: CacheOutcome::Hit,
            ..
        }
    ));
}
