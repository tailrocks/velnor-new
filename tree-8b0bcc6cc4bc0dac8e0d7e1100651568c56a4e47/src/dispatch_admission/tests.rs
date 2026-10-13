use super::*;
use crate::session::completed_report::{CommandRole, SessionIdentity};
use mbx_cache_core::AdapterKind;

fn fixture() -> (tempfile::TempDir, SessionIdentity, AdmissionOwner) {
    let directory = tempfile::tempdir().unwrap();
    let identity = SessionIdentity::new(CommandRole::CargoBuild, None, None, None).unwrap();
    let owner = AdmissionOwner::open(directory.path(), &identity).unwrap();
    (directory, identity, owner)
}

fn enroll(directory: &std::path::Path, identity: &SessionIdentity) -> AdmissionEntry {
    AdmissionEntry::enroll(
        directory,
        &identity.session_id,
        &identity.root_session_id,
        "0123456789abcdef0123456789abcdef",
        AdapterKind::Rustc,
        AdmissionKind::Process,
    )
    .unwrap()
    .unwrap()
}

#[test]
fn acknowledged_entry_retains_exact_identity_and_digest() {
    let (directory, identity, owner) = fixture();
    enroll(directory.path(), &identity)
        .finish(TerminalOutcome::Acknowledged {
            event_sha256: "a".repeat(64),
        })
        .unwrap();
    let closure = owner.close().unwrap();
    assert!(closure.closed_successfully());
    assert!(closure.identity_matches(&identity.session_id, &identity.root_session_id));
    assert_eq!(closure.scope(), "mbx_session_admissions");
    assert_eq!(closure.lifetime(), "accepted_before_close");
    assert_eq!(closure.accepted_entries().count(), 1);
    assert_eq!(closure.ledger_inventory_sha256().len(), 64);
}

#[test]
fn close_retains_outstanding_entry_and_rejects_late_enrollment() {
    let (directory, identity, owner) = fixture();
    let entry = enroll(directory.path(), &identity);
    let closure = owner.close().unwrap();
    assert!(!closure.closed_successfully());
    assert!(
        entry
            .finish(TerminalOutcome::Acknowledged {
                event_sha256: "a".repeat(64)
            })
            .is_err()
    );
    assert!(
        AdmissionEntry::enroll(
            directory.path(),
            &identity.session_id,
            &identity.root_session_id,
            "abcdef0123456789abcdef0123456789",
            AdapterKind::Rustc,
            AdmissionKind::Process
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn failed_delivery_never_becomes_closed_success() {
    let (directory, identity, owner) = fixture();
    enroll(directory.path(), &identity)
        .finish(TerminalOutcome::Failed {
            reason: "actual_receipt_failure".into(),
        })
        .unwrap();
    assert!(!owner.close().unwrap().closed_successfully());
}

#[test]
fn generic_empty_seal_declares_only_accepted_lifetime() {
    let (_directory, _identity, owner) = fixture();
    let closure = owner.close().unwrap();
    assert_eq!(closure.accepted_entries().count(), 0);
    assert_eq!(closure.scope(), "mbx_session_admissions");
    assert_eq!(closure.lifetime(), "accepted_before_close");
    // This token cannot assert enabled routes or whole-task/descendant closure.
    let serialized = serde_json::to_value(&closure).unwrap();
    assert!(serialized.get("routes_closed").is_none());
    assert!(serialized.get("task_complete").is_none());
}
