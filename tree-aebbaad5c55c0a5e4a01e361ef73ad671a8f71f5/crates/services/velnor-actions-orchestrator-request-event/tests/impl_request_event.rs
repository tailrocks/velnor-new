//! Event resolution: names, fork flags, and ref selection.
use velnor_actions_contract_workflow::WorkflowEvent;
use velnor_actions_orchestrator_request_event::request_event::{request_refs, workflow_event_for};

fn pr_payload(fork: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "pull_request": {
            "head": {"repo": {"fork": fork}, "sha": "head-sha"},
            "base": {"sha": "base-sha"},
        },
    })
}

#[test]
fn fork_pr_resolves_to_fork() {
    assert_eq!(
        workflow_event_for("pull_request", &pr_payload(serde_json::json!(true)))
            .expect("fork resolves"),
        WorkflowEvent::Fork
    );
}

#[test]
fn same_repo_pr_resolves_to_pull_request() {
    assert_eq!(
        workflow_event_for("pull_request", &pr_payload(serde_json::json!(false)))
            .expect("pr resolves"),
        WorkflowEvent::PullRequest
    );
}

#[test]
fn missing_fork_flag_is_indeterminate() {
    let payload = serde_json::json!({"pull_request": {"head": {"repo": {}}, "base": {"sha": "b"}}});
    let err = workflow_event_for("pull_request", &payload).expect_err("missing flag refuses");
    assert!(err.to_string().contains("indeterminate_fork"), "{err}");
}

#[test]
fn non_boolean_fork_flag_is_indeterminate() {
    for fork in [serde_json::json!("yes"), serde_json::json!(1)] {
        let err =
            workflow_event_for("pull_request", &pr_payload(fork)).expect_err("non-bool refuses");
        assert!(err.to_string().contains("indeterminate_fork"), "{err}");
    }
}

#[test]
fn direct_events_map_by_name() {
    let empty = serde_json::json!({});
    for (name, event) in [
        ("push", WorkflowEvent::Push),
        ("merge_group", WorkflowEvent::MergeGroup),
        ("local", WorkflowEvent::Local),
    ] {
        assert_eq!(workflow_event_for(name, &empty).expect("maps"), event);
    }
}

#[test]
fn comment_triggers_are_unsupported() {
    let empty = serde_json::json!({});
    for name in ["issue_comment", "pull_request_review_comment", "schedule"] {
        let err = workflow_event_for(name, &empty).expect_err("unsupported refuses");
        assert!(err.to_string().contains("unsupported_event"), "{err}");
    }
}

#[test]
fn pr_refs_read_base_and_head() {
    let (base, head) = request_refs(
        WorkflowEvent::PullRequest,
        &pr_payload(serde_json::json!(false)),
        None,
    )
    .expect("pr refs");
    assert_eq!(base.as_deref(), Some("base-sha"));
    assert_eq!(head, "head-sha");
    let (base, head) = request_refs(
        WorkflowEvent::Fork,
        &pr_payload(serde_json::json!(true)),
        None,
    )
    .expect("fork refs");
    assert_eq!(base.as_deref(), Some("base-sha"));
    assert_eq!(head, "head-sha");
}

#[test]
fn pr_refs_require_both_shas() {
    let no_base = serde_json::json!({"pull_request": {"head": {"sha": "h"}}});
    let err = request_refs(WorkflowEvent::PullRequest, &no_base, None).expect_err("no base");
    assert!(err.to_string().contains("missing_pr_base"), "{err}");
    let no_head = serde_json::json!({"pull_request": {"base": {"sha": "b"}}});
    let err = request_refs(WorkflowEvent::PullRequest, &no_head, None).expect_err("no head");
    assert!(err.to_string().contains("missing_pr_head"), "{err}");
}

#[test]
fn merge_group_refs_read_group_shas() {
    let payload = serde_json::json!({"merge_group": {"base_sha": "b", "head_sha": "h"}});
    let (base, head) = request_refs(WorkflowEvent::MergeGroup, &payload, None).expect("group refs");
    assert_eq!(base.as_deref(), Some("b"));
    assert_eq!(head, "h");
    let err = request_refs(WorkflowEvent::MergeGroup, &serde_json::json!({}), None)
        .expect_err("empty refuses");
    assert!(err.to_string().contains("missing_merge_base"), "{err}");
}

#[test]
fn push_refs_prefer_payload_shas() {
    let payload = serde_json::json!({"before": "b", "after": "a"});
    let (base, head) = request_refs(WorkflowEvent::Push, &payload, Some("gha")).expect("push refs");
    assert_eq!(base.as_deref(), Some("b"));
    assert_eq!(head, "a");
}

#[test]
fn push_refs_skip_zero_shas() {
    let zero = "0".repeat(40);
    let payload = serde_json::json!({"before": zero, "after": zero});
    let (base, head) =
        request_refs(WorkflowEvent::Push, &payload, Some("gha")).expect("zero falls back");
    assert_eq!(base, None);
    assert_eq!(head, "gha");
    let err = request_refs(WorkflowEvent::Push, &payload, None).expect_err("no head refuses");
    assert!(err.to_string().contains("missing_push_head"), "{err}");
}

#[test]
fn local_refs_default_head() {
    let (base, head) =
        request_refs(WorkflowEvent::Local, &serde_json::json!({}), None).expect("local refs");
    assert_eq!(base, None);
    assert_eq!(head, "HEAD");
    let (_, head) =
        request_refs(WorkflowEvent::Local, &serde_json::json!({}), Some("sha")).expect("local sha");
    assert_eq!(head, "sha");
}
