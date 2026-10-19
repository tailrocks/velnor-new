//! Event hints are data, never arbitrary PR selectors or trusted review evidence.
use super::*;
use serde_json::json;

#[test]
fn only_pr_target_and_pr_issue_events_supply_positive_hints() {
    for action in ["opened", "synchronize", "reopened", "ready_for_review"] {
        assert_eq!(
            hint(
                "pull_request_target",
                &json!({"action":action,"pull_request":{"number":12}})
            )
            .expect("PR"),
            Some(12)
        );
    }
    for action in ["created", "edited", "deleted"] {
        assert_eq!(
            hint(
                "issue_comment",
                &json!({"action":action,"issue":{"number":12,"pull_request":{}}})
            )
            .expect("PR comment"),
            Some(12)
        );
        assert_eq!(
            hint(
                "issue_comment",
                &json!({"action":action,"issue":{"number":12}})
            )
            .expect("ordinary issue"),
            None
        );
    }
    for event in [
        "push",
        "pull_request",
        "check_run",
        "workflow_dispatch",
        "schedule",
    ] {
        assert!(
            hint(
                event,
                &json!({"action":"opened","pull_request":{"number":12}})
            )
            .is_err()
        );
    }
    assert!(
        hint(
            "pull_request_target",
            &json!({"action":"opened","pull_request":{"number":0}})
        )
        .is_err()
    );
    assert!(
        hint(
            "pull_request_target",
            &json!({"action":"closed","pull_request":{"number":12}})
        )
        .is_err()
    );
}
