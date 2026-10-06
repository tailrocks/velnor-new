//! An empty controller is waiting for credentials, never ready.

use crate::{Readiness, doctor_json, readiness_for_empty, status_json};

#[test]
fn empty_controller_is_not_ready() {
    let state = readiness_for_empty();
    assert_eq!(state, Readiness::WaitingForCredentials);
    assert_eq!(state.as_str(), "waiting_for_credentials");
    assert_ne!(state, Readiness::Ready);
    let status = status_json(state);
    let probed = doctor_json(state, true);
    let quiet = doctor_json(state, false);
    assert_eq!(status, r#"{"state":"waiting_for_credentials"}"#);
    assert_eq!(
        probed,
        r#"{"state":"waiting_for_credentials","probe":true}"#
    );
    assert_eq!(
        quiet,
        r#"{"state":"waiting_for_credentials","probe":false}"#
    );
    assert!(!status.contains("\"ready\""));
    assert!(!probed.contains("\"ready\""));
    assert!(!quiet.contains("\"ready\""));
}
