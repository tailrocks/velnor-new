use super::*;
use serde_json::json;

fn event() -> Value {
    json!({
        "action": "completed",
        "repository": {"id": 9, "full_name": "jackin-project/jackin"},
        "workflow_run": {
            "id": 101,
            "run_attempt": 2,
            "head_sha": "a".repeat(40),
            "head_branch": "main",
            "event": "push",
            "status": "completed",
            "conclusion": "success",
            "path": velnor_actions_contract::CI_WORKFLOW_PATH,
            "name": velnor_actions_contract::WORKFLOW_DISPLAY_NAME,
            "head_repository": {"id": 9}
        }
    })
}

#[test]
fn completed_main_push_ci_event_admits_source_identity_a() {
    assert_eq!(
        source_from_workflow_run_event(&event(), 9, "jackin-project/jackin")
            .expect("authenticated event shape"),
        ("a".repeat(40), 101, 2)
    );
}

#[test]
fn source_event_rejects_foreign_repositories_wrong_workflow_and_non_success() {
    for (pointer, replacement) in [
        ("/repository/id", json!(10)),
        ("/repository/full_name", json!("attacker/fork")),
        ("/workflow_run/head_repository/id", json!(10)),
        ("/workflow_run/head_branch", json!("feature")),
        ("/workflow_run/event", json!("pull_request")),
        ("/workflow_run/status", json!("in_progress")),
        ("/workflow_run/conclusion", json!("failure")),
        ("/workflow_run/path", json!(".github/workflows/other.yml")),
        ("/workflow_run/name", json!("Untrusted CI")),
        ("/workflow_run/head_sha", json!("A".repeat(40))),
    ] {
        let mut candidate = event();
        *candidate.pointer_mut(pointer).expect("mutation field") = replacement;
        assert!(
            source_from_workflow_run_event(&candidate, 9, "jackin-project/jackin").is_err(),
            "accepted modified source event"
        );
    }
}
