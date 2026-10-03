use super::*;
use crate::dispatch_admission::AdmissionOwner;
use crate::session::completed_report::{CommandRole, SessionIdentity};
use mbx_cache_core::{CacheOutcome, InvocationKind, OutputObservation};

fn fixture() -> (tempfile::TempDir, PathBuf, DeliveryIdentity, AdmissionOwner) {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().canonicalize().unwrap();
    let session = SessionIdentity::new(CommandRole::Exec, None, None, None).unwrap();
    let owner = AdmissionOwner::open(&directory, &session).unwrap();
    let identity = DeliveryIdentity {
        session_id: session.session_id,
        root_session_id: session.root_session_id,
        parent_session_id: session.parent_session_id,
        caller_correlation: None,
    };
    (temporary, directory, identity, owner)
}

fn invocation_event() -> MeasurementEvent {
    MeasurementEvent::Invocation {
        adapter: AdapterKind::Rustc,
        invocation_kind: InvocationKind::Work,
        cache_outcome: CacheOutcome::Miss,
        unit: None,
    }
}

#[test]
fn receipt_and_admission_terminal_share_event_id_and_payload_digest() {
    let (_temporary, path, identity, owner) = fixture();
    let delivery =
        Delivery::enroll_at(path, identity, AdapterKind::Rustc, EventKind::Invocation).unwrap();
    let event_id = delivery.event_id.clone();
    let event = invocation_event();
    let digest = terminal_event_digest(&event).unwrap();
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(delivery.acknowledge(&event).unwrap()).unwrap())
            .unwrap();
    assert_eq!(receipt["event_id"], event_id);
    assert_eq!(receipt["delivery"]["event_sha256"], digest);
    let closure = owner.close().unwrap();
    assert!(closure.closed_successfully());
    let entries: Vec<_> = closure.accepted_entries().collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].event_id, event_id);
    assert!(
        matches!(&entries[0].terminal, Some(TerminalOutcome::Acknowledged{event_sha256}) if event_sha256 == &digest)
    );
}

#[test]
fn failed_receipt_write_leaves_admission_outstanding() {
    let (_temporary, path, identity, owner) = fixture();
    let delivery = Delivery::enroll_at(
        path.clone(),
        identity,
        AdapterKind::Rustc,
        EventKind::Invocation,
    )
    .unwrap();
    std::fs::create_dir(path.join(format!(
        "{}.{}.measurement-failure.json",
        delivery.identity.session_id, delivery.event_id
    )))
    .unwrap();
    assert!(delivery.fail(DeliveryFailure::RequestFailed).is_err());
    let closure = owner.close().unwrap();
    assert!(!closure.closed_successfully());
    let value = serde_json::to_value(closure).unwrap();
    assert_eq!(value["accepted_count"], 1);
    assert_eq!(value["outstanding_count"], 1);
    assert_eq!(value["acknowledged_count"], 0);
}

#[test]
fn dropped_delivery_never_invents_a_terminal() {
    let (_temporary, path, identity, owner) = fixture();
    drop(Delivery::enroll_at(path, identity, AdapterKind::Rustc, EventKind::Invocation).unwrap());
    let value = serde_json::to_value(owner.close().unwrap()).unwrap();
    assert_eq!(value["outstanding_count"], 1);
    assert_eq!(value["failed_count"], 0);
}

#[test]
fn late_enrollment_is_typed_unavailable_without_new_attempt() {
    let (_temporary, path, identity, owner) = fixture();
    assert!(owner.close().unwrap().closed_successfully());
    let error = Delivery::enroll_at(
        path.clone(),
        identity,
        AdapterKind::Rustc,
        EventKind::Invocation,
    )
    .err()
    .unwrap();
    assert!(error.downcast_ref::<AdmissionClosed>().is_some());
    assert!(std::fs::read_dir(path).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("measurement-attempt")
    }));
}

#[test]
fn output_receipt_closes_its_actual_output_admission() {
    let (_temporary, path, identity, owner) = fixture();
    let event = MeasurementEvent::Output {
        adapter: AdapterKind::Rustc,
        unit: None,
        observation: OutputObservation {
            path: "/actual-output".into(),
            aliases: Vec::new(),
            cache_outcome: CacheOutcome::Hit,
            digest: mbx_cache_core::CacheDigest::blake3(b"output"),
            file_identity: None,
        },
    };
    Delivery::enroll_at(path, identity, AdapterKind::Rustc, EventKind::Output)
        .unwrap()
        .acknowledge(&event)
        .unwrap();
    let closure = owner.close().unwrap();
    assert!(closure.closed_successfully());
    assert!(matches!(
        closure.accepted_entries().next().unwrap().kind,
        AdmissionKind::Output
    ));
}
