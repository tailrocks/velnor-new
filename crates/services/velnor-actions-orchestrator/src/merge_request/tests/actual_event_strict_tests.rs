use super::super::*;

#[test]
fn duplicate_payload_keys_fail_closed() {
    let mut errors = Vec::new();
    let dup = r#"{"pull_request":{"head":{"repo":{"fork":false}}},"pull_request":{}}"#;
    assert!(resolve_actual_event(Some("pull_request"), Some(dup), &mut errors).is_none());
    assert!(
        errors.iter().any(|err| err == "malformed_actual_payload"),
        "{errors:?}"
    );
    let mut errors = Vec::new();
    let valid = r#"{"pull_request":{"head":{"repo":{"fork":true}}}}"#;
    assert!(resolve_actual_event(Some("pull_request"), Some(valid), &mut errors).is_some());
    assert!(errors.is_empty(), "{errors:?}");
}
