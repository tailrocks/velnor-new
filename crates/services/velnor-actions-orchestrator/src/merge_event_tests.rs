//! Assembly event/attestation tests.
//!
//! Declared via `#[path]` from `merge_request.rs` under `cfg(test)`;
//! staging helpers live in `merge_request_tests`.

use super::merge_request_tests::{error_list, plan_with, staged};
use super::*;

#[test]
fn assembly_captures_actual_event() {
    let dir = staged(&plan_with(&[]), &[]);
    let needs = Some(r#"{"plan":"success"}"#);
    let expected = Some(r#"["plan"]"#);
    let actual_of = |request: &str| {
        serde_json::from_str::<serde_json::Value>(request).expect("json")["actual_event"].clone()
    };
    for (name, want) in [
        ("push", "push"),
        ("merge_group", "merge_group"),
        ("local", "local"),
    ] {
        let request = assemble_with_needs("local", dir.path(), needs, expected, Some(name), None)
            .expect("assemble");
        assert_eq!(actual_of(&request), serde_json::json!(want), "{name}");
        assert!(error_list(&request).is_empty(), "{request}");
    }
    for (fork, want) in [(true, "fork"), (false, "pull_request")] {
        let payload = format!(r#"{{"pull_request":{{"head":{{"repo":{{"fork":{fork}}}}}}}}}"#);
        let request = assemble_with_needs(
            "local",
            dir.path(),
            needs,
            expected,
            Some("pull_request"),
            Some(&payload),
        )
        .expect("assemble");
        assert_eq!(actual_of(&request), serde_json::json!(want), "fork={fork}");
        assert!(error_list(&request).is_empty(), "{request}");
    }
    for (name, payload, want) in [
        (None, None, "missing_actual_event"),
        (Some("push"), Some("not json"), "malformed_actual_payload"),
        (Some("schedule"), Some("{}"), "unsupported_event"),
        (Some("pull_request"), None, "missing_actual_payload"),
        (Some("pull_request"), Some("{}"), "indeterminate_fork"),
    ] {
        let request = assemble_with_needs("local", dir.path(), needs, expected, name, payload)
            .expect("assemble");
        assert!(actual_of(&request).is_null(), "{name:?}");
        assert!(
            error_list(&request).contains(&want.to_owned()),
            "{name:?}: {}",
            error_list(&request).join(",")
        );
    }
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
