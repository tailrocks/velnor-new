use super::*;
#[cfg(unix)]
use mbx_cache_core::{CacheOutcome, InvocationKind};

fn identity() -> DeliveryIdentity {
    DeliveryIdentity {
        session_id: "11111111-1111-4111-8111-111111111111".into(),
        root_session_id: "22222222-2222-4222-8222-222222222222".into(),
        parent_session_id: Some("22222222-2222-4222-8222-222222222222".into()),
        caller_correlation: Some("public-task:alpha".into()),
    }
}

#[cfg(unix)]
fn event() -> MeasurementEvent {
    MeasurementEvent::Invocation {
        adapter: AdapterKind::Rustc,
        invocation_kind: InvocationKind::Work,
        cache_outcome: CacheOutcome::Miss,
        unit: None,
    }
}

fn directory() -> (tempfile::TempDir, PathBuf) {
    let temporary = tempfile::tempdir().unwrap();
    let canonical = temporary.path().canonicalize().unwrap();
    if cfg!(all(unix, feature = "owned-cache-transport")) {
        let delivery = identity();
        let session = crate::session::completed_report::SessionIdentity {
            session_id: delivery.session_id,
            root_session_id: delivery.root_session_id,
            parent_session_id: delivery.parent_session_id,
            command_role: crate::session::completed_report::CommandRole::Other,
            caller_correlation: delivery.caller_correlation,
        };
        crate::dispatch_admission::AdmissionOwner::open(&canonical, &session).unwrap();
    }
    (temporary, canonical)
}

fn evidence_files(path: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .contains(".measurement-")
        })
        .collect()
}

#[cfg(unix)]
fn read(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[cfg(unix)]
#[test]
fn attempt_and_ack_are_distinct_immutable_correlated_native_evidence() {
    let (_temporary, path) = directory();
    let delivery = Delivery::enroll_at(
        path.clone(),
        identity(),
        AdapterKind::Rustc,
        EventKind::Invocation,
    )
    .unwrap();
    let attempt = delivery.publish(Stage::Attempted);
    assert!(attempt.is_err(), "an immutable attempt cannot be replaced");
    let files = evidence_files(&path);
    assert_eq!(files.len(), 1);
    let attempt = read(&files[0]);
    assert_eq!(attempt["schema_version"], 1);
    assert_eq!(attempt["delivery"]["status"], "attempted");
    assert_eq!(attempt["adapter"], "rustc");
    assert_eq!(attempt["event_kind"], "invocation");
    assert_eq!(attempt["identity"]["session_id"], identity().session_id);
    assert_eq!(
        attempt["identity"]["root_session_id"],
        identity().root_session_id
    );
    assert_eq!(
        attempt["identity"]["parent_session_id"],
        identity().parent_session_id.unwrap()
    );
    assert_eq!(
        attempt["identity"]["caller_correlation"],
        "public-task:alpha"
    );
    assert_eq!(
        attempt["source_base_version"],
        crate::version::SOURCE_BASE_VERSION
    );
    let acknowledged = read(&delivery.acknowledge(&event()).unwrap());
    assert_eq!(acknowledged["delivery"]["status"], "acknowledged");
    assert_eq!(acknowledged["event_id"], attempt["event_id"]);
    assert_eq!(
        acknowledged["delivery"]["event"],
        serde_json::to_value(event()).unwrap()
    );
    assert_eq!(
        acknowledged["delivery"]["event_sha256"],
        "2658a3165d5cda8e77e38a30a85dd7d10e1d3adc35e3ca6242099307cdd4344d"
    );
    assert_eq!(evidence_files(&path).len(), 2);
}

#[cfg(unix)]
#[test]
fn lost_ack_leaves_attempt_and_explicit_failure_without_success_receipt() {
    let (_temporary, path) = directory();
    let delivery = Delivery::enroll_at(
        path.clone(),
        identity(),
        AdapterKind::Rustc,
        EventKind::Invocation,
    )
    .unwrap();
    let failure = read(&delivery.fail(DeliveryFailure::RequestFailed).unwrap());
    assert_eq!(failure["delivery"]["status"], "failed");
    assert_eq!(failure["delivery"]["reason"], "request_failed");
    assert_eq!(evidence_files(&path).len(), 2);
    assert!(std::fs::read_dir(path).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("measurement-ack")
    }));
}

#[cfg(unix)]
#[test]
fn receipt_write_failure_preserves_attempt_instead_of_forging_zero() {
    let (_temporary, path) = directory();
    let delivery = Delivery::enroll_at(
        path.clone(),
        identity(),
        AdapterKind::Rustc,
        EventKind::Invocation,
    )
    .unwrap();
    let destination = path.join(format!(
        "{}.{}.measurement-failure.json",
        delivery.identity.session_id, delivery.event_id
    ));
    std::fs::create_dir(&destination).unwrap();
    assert!(delivery.fail(DeliveryFailure::RequestFailed).is_err());
    let attempts: Vec<_> = std::fs::read_dir(&path)
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.unwrap();
            entry
                .file_name()
                .to_string_lossy()
                .contains("measurement-attempt")
                .then(|| entry.path())
        })
        .collect();
    assert_eq!(attempts.len(), 1);
    assert_eq!(read(&attempts[0])["delivery"]["status"], "attempted");
}

#[test]
fn invalid_identity_cannot_publish_evidence() {
    let (_temporary, path) = directory();
    let mut invalid = identity();
    invalid.session_id = "../escape".into();
    assert!(
        Delivery::enroll_at(
            path.clone(),
            invalid,
            AdapterKind::Rustc,
            EventKind::Invocation
        )
        .is_err()
    );
    let mut invalid = identity();
    invalid.caller_correlation = Some("private argument".into());
    assert!(
        Delivery::enroll_at(
            path.clone(),
            invalid,
            AdapterKind::Rustc,
            EventKind::Invocation
        )
        .is_err()
    );
    let mut invalid = identity();
    invalid.parent_session_id = None;
    assert!(
        Delivery::enroll_at(
            path.clone(),
            invalid,
            AdapterKind::Rustc,
            EventKind::Invocation
        )
        .is_err()
    );
    let mut invalid = identity();
    invalid.parent_session_id = Some(invalid.session_id.clone());
    assert!(
        Delivery::enroll_at(
            path.clone(),
            invalid,
            AdapterKind::Rustc,
            EventKind::Invocation
        )
        .is_err()
    );
    let mut invalid = identity();
    invalid.root_session_id = invalid.session_id.clone();
    assert!(
        Delivery::enroll_at(
            path.clone(),
            invalid,
            AdapterKind::Rustc,
            EventKind::Invocation
        )
        .is_err()
    );
    assert_eq!(evidence_files(&path).len(), 0);
}

#[cfg(unix)]
#[test]
fn artifacts_are_read_only_and_symlink_destinations_are_rejected() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let (_temporary, path) = directory();
    let delivery = Delivery::enroll_at(
        path.clone(),
        identity(),
        AdapterKind::Rustc,
        EventKind::Invocation,
    )
    .unwrap();
    let ack = delivery.acknowledge(&event()).unwrap();
    assert_eq!(
        std::fs::metadata(ack).unwrap().permissions().mode() & 0o777,
        0o400
    );
    let alias = path.join("alias");
    symlink(&path, &alias).unwrap();
    assert!(
        Delivery::enroll_at(alias, identity(), AdapterKind::Rustc, EventKind::Invocation).is_err()
    );
}

#[cfg(not(unix))]
#[test]
fn unsupported_private_evidence_platform_fails_before_filesystem_work() {
    assert!(
        Delivery::enroll_at(
            PathBuf::new(),
            identity(),
            AdapterKind::Rustc,
            EventKind::Invocation
        )
        .is_err()
    );
}

#[cfg(unix)]
#[test]
fn acknowledgement_for_another_adapter_cannot_publish_orphan_receipt() {
    let (_temporary, path) = directory();
    let delivery = Delivery::enroll_at(
        path.clone(),
        identity(),
        AdapterKind::Rustc,
        EventKind::Invocation,
    )
    .unwrap();
    let other = MeasurementEvent::Invocation {
        adapter: AdapterKind::Cc,
        invocation_kind: InvocationKind::Work,
        cache_outcome: CacheOutcome::Miss,
        unit: None,
    };
    assert!(delivery.acknowledge(&other).is_err());
    assert_eq!(evidence_files(&path).len(), 1);
}

#[test]
fn terminal_payload_digest_changes_when_count_or_wall_is_tampered() {
    use mbx_cache_core::{ProcessMeasurement, ProcessOutcome, ProcessPurpose};
    let mut event = MeasurementEvent::Process {
        adapter: AdapterKind::Rustc,
        purpose: ProcessPurpose::Work,
        outcome: ProcessOutcome::Succeeded,
        measurement: ProcessMeasurement {
            attempts: 1,
            started: 1,
            observed_wall_ns: 47,
            wall_observations: 1,
        },
        unit: None,
    };
    let original = terminal_event_digest(&event).unwrap();
    if let MeasurementEvent::Process { measurement, .. } = &mut event {
        measurement.attempts = 2;
    }
    assert_ne!(terminal_event_digest(&event).unwrap(), original);
    if let MeasurementEvent::Process { measurement, .. } = &mut event {
        measurement.attempts = 1;
        measurement.observed_wall_ns = 48;
    }
    assert_ne!(terminal_event_digest(&event).unwrap(), original);
}
