use super::*;
use serde_json::{Value, json};

#[test]
fn completed_main_push_event_binds_source_to_admitted_ci_workflow() {
    assert_eq!(
        source_from_workflow_run_event(&event(), 9, "jackin-project/jackin", &workflow())
            .expect("authenticated event shape"),
        ("a".repeat(40), 101, 2)
    );
}

#[test]
fn source_event_rejects_each_independent_workflow_identity_mutation() {
    for (pointer, replacement) in [
        ("/action", json!("requested")),
        ("/repository/id", json!(10)),
        ("/repository/full_name", json!("attacker/fork")),
        ("/workflow_run/repository/id", json!(10)),
        ("/workflow_run/repository/full_name", json!("attacker/fork")),
        ("/workflow/id", json!(8)),
        ("/workflow/name", json!("Untrusted CI")),
        ("/workflow/path", json!(".github/workflows/other.yml")),
        ("/workflow_run/workflow_id", json!(8)),
        ("/workflow_run/head_repository/id", json!(10)),
        (
            "/workflow_run/head_repository/full_name",
            json!("attacker/fork"),
        ),
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
            source_from_workflow_run_event(&candidate, 9, "jackin-project/jackin", &workflow())
                .is_err(),
            "accepted modified field {pointer}"
        );
    }
}

#[test]
fn source_event_rejects_malformed_envelopes_and_unadmitted_workflow_metadata() {
    for candidate in [
        Value::Null,
        json!({}),
        json!({"action":"completed","workflow_run":null}),
        {
            let mut candidate = event();
            candidate["workflow"] = Value::Null;
            candidate
        },
        {
            let mut candidate = event();
            candidate["workflow_run"] = json!({});
            candidate
        },
    ] {
        assert!(
            source_from_workflow_run_event(&candidate, 9, "jackin-project/jackin", &workflow())
                .is_err(),
            "accepted malformed event envelope"
        );
    }

    let mut wrong_workflow = workflow();
    wrong_workflow.id = 0;
    assert!(
        source_from_workflow_run_event(&event(), 9, "jackin-project/jackin", &wrong_workflow)
            .is_err()
    );
    let mut wrong_workflow = workflow();
    wrong_workflow.path = ".github/workflows/other.yml".to_owned();
    assert!(
        source_from_workflow_run_event(&event(), 9, "jackin-project/jackin", &wrong_workflow)
            .is_err()
    );
    let mut wrong_workflow = workflow();
    wrong_workflow.name = "Untrusted CI".to_owned();
    assert!(
        source_from_workflow_run_event(&event(), 9, "jackin-project/jackin", &wrong_workflow)
            .is_err()
    );
}
