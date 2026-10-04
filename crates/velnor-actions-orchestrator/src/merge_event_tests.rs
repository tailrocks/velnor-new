//! Assembly event/attestation tests.
//!
//! Declared via `#[path]` from `merge_request.rs` under `cfg(test)`;
//! staging helpers live in `merge_request_tests`.

use std::path::Path;

use super::merge_request_tests::{error_list, plan_with, staged};
use super::*;

fn event_value(
    dir: &Path,
    event_name: Option<&str>,
    event_payload: Option<&str>,
) -> serde_json::Value {
    let request = assemble_with_needs(
        "local",
        dir,
        Some(r#"{"plan":"success"}"#),
        Some(r#"["plan"]"#),
        event_name,
        event_payload,
    )
    .expect("assemble");
    serde_json::from_str(&request).expect("json")
}

fn event_errors(value: &serde_json::Value) -> Vec<String> {
    value["assembly_errors"]
        .as_array()
        .expect("assembly errors array")
        .iter()
        .map(|error| error.as_str().expect("assembly error string").to_owned())
        .collect()
}

#[test]
fn assembly_captures_actual_event() {
    let dir = staged(&plan_with(&[]), &[]);
    for (name, want, scope) in [
        ("push", "push", "affected"),
        ("merge_group", "merge_group", "affected"),
        ("local", "local", "affected"),
    ] {
        let value = event_value(dir.path(), Some(name), None);
        assert_eq!(value["actual_event"], serde_json::json!(want), "{name}");
        assert_eq!(value["actual_scope"], serde_json::json!(scope), "{name}");
        assert!(event_errors(&value).is_empty(), "{value}");
    }
    for (fork, want) in [(true, "fork"), (false, "pull_request")] {
        let payload = format!(r#"{{"pull_request":{{"head":{{"repo":{{"fork":{fork}}}}}}}}}"#);
        let value = event_value(dir.path(), Some("pull_request"), Some(&payload));
        assert_eq!(
            value["actual_event"],
            serde_json::json!(want),
            "fork={fork}"
        );
        assert_eq!(
            value["actual_scope"],
            serde_json::json!("affected"),
            "fork={fork}"
        );
        assert!(event_errors(&value).is_empty(), "{value}");
    }
}

#[test]
fn assembly_captures_full_schedule_and_dispatch_scope() {
    let dir = staged(&plan_with(&[]), &[]);
    let schedule = event_value(dir.path(), Some("schedule"), Some("{}"));
    assert_eq!(schedule["actual_event"], serde_json::json!("schedule"));
    assert_eq!(schedule["actual_scope"], serde_json::json!("full"));
    assert!(event_errors(&schedule).is_empty(), "{schedule}");

    for (payload, scope) in [
        (r#"{"inputs":{}}"#, "affected"),
        (r#"{"inputs":{"scope":"affected"}}"#, "affected"),
        (r#"{"inputs":{"scope":"full"}}"#, "full"),
    ] {
        let value = event_value(dir.path(), Some("workflow_dispatch"), Some(payload));
        assert_eq!(
            value["actual_event"],
            serde_json::json!("workflow_dispatch"),
            "{payload}"
        );
        assert_eq!(value["actual_scope"], serde_json::json!(scope), "{payload}");
        assert!(event_errors(&value).is_empty(), "{value}");
    }
}

#[test]
fn assembly_rejects_missing_or_malformed_events() {
    let dir = staged(&plan_with(&[]), &[]);
    for (name, payload, want) in [
        (None, None, "missing_actual_event"),
        (Some("push"), Some("not json"), "malformed_actual_payload"),
        (Some("issue_comment"), Some("{}"), "unsupported_event"),
        (Some("pull_request"), None, "missing_actual_payload"),
        (Some("workflow_dispatch"), None, "missing_actual_payload"),
        (Some("pull_request"), Some("{}"), "indeterminate_fork"),
    ] {
        let value = event_value(dir.path(), name, payload);
        assert!(value["actual_event"].is_null(), "{name:?}");
        assert!(
            event_errors(&value).contains(&want.to_owned()),
            "{name:?}: {}",
            event_errors(&value).join(",")
        );
    }
}

#[test]
fn assembly_rejects_malformed_dispatch_scope_and_duplicates() {
    let dir = staged(&plan_with(&[]), &[]);
    let typed_scope = event_value(
        dir.path(),
        Some("workflow_dispatch"),
        Some(r#"{"inputs":{"scope":true}}"#),
    );
    assert_eq!(
        typed_scope["actual_event"],
        serde_json::json!("workflow_dispatch")
    );
    assert!(typed_scope["actual_scope"].is_null(), "{typed_scope}");
    assert!(
        event_errors(&typed_scope).contains(&"malformed_scope".to_owned()),
        "{typed_scope}"
    );

    let duplicate_scope = event_value(
        dir.path(),
        Some("workflow_dispatch"),
        Some(r#"{"inputs":{"scope":"full","scope":"affected"}}"#),
    );
    assert!(
        duplicate_scope["actual_event"].is_null(),
        "{duplicate_scope}"
    );
    assert!(
        duplicate_scope["actual_scope"].is_null(),
        "{duplicate_scope}"
    );
    assert!(
        event_errors(&duplicate_scope).contains(&"malformed_actual_payload".to_owned()),
        "{duplicate_scope}"
    );
}

#[test]
fn assembly_reads_attestation_in_candidate_mode() {
    let file = |body: &str| {
        staged(
            &plan_with(&[]),
            &[("candidate/candidate-attestation.json", body)],
        )
    };
    let needs = Some(r#"{"candidate":"success","plan":"success"}"#);
    let expected = Some(r#"["candidate","plan"]"#);
    let dir = file(r#"{"schema":1,"commit":"abc"}"#);
    let request = assemble_with_needs(
        "local",
        dir.path(),
        needs,
        expected,
        Some("push"),
        Some("{}"),
    )
    .expect("assemble");
    let value: serde_json::Value = serde_json::from_str(&request).expect("json");
    assert_eq!(value["candidate_attestation"]["commit"], "abc");
    assert!(error_list(&request).is_empty(), "{request}");

    let bare = staged(&plan_with(&[]), &[]);
    let request = assemble_with_needs(
        "local",
        bare.path(),
        needs,
        expected,
        Some("push"),
        Some("{}"),
    )
    .expect("assemble");
    let value: serde_json::Value = serde_json::from_str(&request).expect("json");
    assert!(value["candidate_attestation"].is_null(), "{request}");
    assert!(
        error_list(&request).contains(&"missing_candidate_attestation".to_owned()),
        "{request}"
    );

    let dir = file("not json");
    let request = assemble_with_needs(
        "local",
        dir.path(),
        needs,
        expected,
        Some("push"),
        Some("{}"),
    )
    .expect("assemble");
    assert!(
        error_list(&request).contains(&"unparsable_candidate_attestation".to_owned()),
        "{request}"
    );

    let request = assemble_with_needs(
        "local",
        bare.path(),
        Some(r#"{"plan":"success"}"#),
        Some(r#"["plan"]"#),
        Some("push"),
        Some("{}"),
    )
    .expect("assemble");
    let value: serde_json::Value = serde_json::from_str(&request).expect("json");
    assert!(value["candidate_attestation"].is_null(), "{request}");
    assert!(error_list(&request).is_empty(), "{request}");
}
