//! Shorthand branch validation regressions for baseline publication.

use super::baseline_publish_tests::{fixture_plan, refuse_problem, request_json};

#[test]
fn publish_rejects_branch_names_that_could_be_shell_syntax() {
    let head = "a".repeat(40);
    let plan = fixture_plan(&head, "r7-a1");
    let mut request: serde_json::Value =
        serde_json::from_str(&request_json(&head)).expect("request");
    request["default_branch"] = serde_json::json!("main;evil");
    request["git_ref"] = serde_json::json!("refs/heads/main;evil");

    let problem = refuse_problem(&request.to_string(), &plan, "r7-a1");
    assert!(problem.contains("unprotected_ref"), "{problem}");
}
