use super::*;

#[test]
fn schedule_always_uses_full_scope() {
    let payload = serde_json::json!({"inputs": {"scope": "affected"}});
    assert_eq!(
        scope_for(WorkflowEvent::Schedule, &payload).expect("schedule scope"),
        VerificationScope::Full
    );
}

#[test]
fn dispatch_defaults_to_affected() {
    let absent = serde_json::json!({});
    let missing = serde_json::json!({"inputs": {"other": "full"}});
    assert_eq!(
        scope_for(WorkflowEvent::WorkflowDispatch, &absent).expect("absent scope"),
        VerificationScope::Affected
    );
    assert_eq!(
        scope_for(WorkflowEvent::WorkflowDispatch, &missing).expect("missing scope"),
        VerificationScope::Affected
    );
}

#[test]
fn dispatch_accepts_only_exact_scope_values() {
    for (value, expected) in [
        ("affected", VerificationScope::Affected),
        ("full", VerificationScope::Full),
    ] {
        let payload = serde_json::json!({"inputs": {"scope": value}});
        assert_eq!(
            scope_for(WorkflowEvent::WorkflowDispatch, &payload).expect("dispatch scope"),
            expected
        );
    }
}

#[test]
fn dispatch_rejects_malformed_scope_values() {
    for payload in [
        serde_json::json!({"inputs": {"scope": "FULL"}}),
        serde_json::json!({"inputs": {"scope": "everything"}}),
        serde_json::json!({"inputs": {"scope": true}}),
        serde_json::json!({"inputs": []}),
    ] {
        let error = scope_for(WorkflowEvent::WorkflowDispatch, &payload)
            .expect_err("malformed scope must fail closed");
        assert!(error.to_string().contains("malformed_scope"), "{error}");
    }
}

#[test]
fn non_dispatch_events_ignore_inputs() {
    let payload = serde_json::json!({"inputs": {"scope": "invalid"}});
    assert_eq!(
        scope_for(WorkflowEvent::Push, &payload).expect("push scope"),
        VerificationScope::Affected
    );
}
