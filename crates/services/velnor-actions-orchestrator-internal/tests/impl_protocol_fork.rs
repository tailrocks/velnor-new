//! Fork routing: fork pull-request payloads materialize the Fork event.

use std::fs;

use tempfile::TempDir;
use velnor_actions_orchestrator_internal::internal::write_request_parts;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn write_request_routes_fork_pull_requests_to_fork() -> TestResult {
    let dir = TempDir::new()?;
    let base = "b".repeat(64);
    let head = "a".repeat(64);
    for (fork, expected) in [(true, "fork"), (false, "pull_request")] {
        let payload = serde_json::json!({
            "pull_request": {
                "base": {"sha": base},
                "head": {"sha": head, "repo": {"fork": fork}},
            },
        })
        .to_string();
        let file = dir.path().join(expected).join("plan-v1-request.json");
        write_request_parts(&file, "pull_request", &payload, None, None, dir.path())?;
        let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file)?)?;
        assert_eq!(value["event"], expected, "fork={fork}");
        assert_eq!(value["base"], base);
        assert_eq!(value["head"], head);
    }
    Ok(())
}

#[test]
fn write_request_fails_closed_without_a_fork_flag() -> TestResult {
    let dir = TempDir::new()?;
    let base = "b".repeat(40);
    let head = "a".repeat(40);
    for (name, payload) in [
        (
            "missing",
            serde_json::json!({"pull_request": {"base": {"sha": base}, "head": {"sha": head}}}),
        ),
        (
            "non_bool",
            serde_json::json!({"pull_request": {"base": {"sha": base}, "head": {"sha": head, "repo": {"fork": "no"}}}}),
        ),
    ] {
        let file = dir.path().join(name).join("plan-v1-request.json");
        let error = write_request_parts(
            &file,
            "pull_request",
            &payload.to_string(),
            None,
            None,
            dir.path(),
        )
        .expect_err("missing/non-bool fork flag must fail closed");
        assert!(
            error.to_string().contains("indeterminate_fork"),
            "unexpected error for {name}: {error}"
        );
    }
    Ok(())
}

#[test]
fn write_request_rejects_comment_triggers() -> TestResult {
    let dir = TempDir::new()?;
    for event in ["issue_comment", "pull_request_review_comment"] {
        let file = dir.path().join(event).join("plan-v1-request.json");
        let error = write_request_parts(&file, event, "{}", None, None, dir.path())
            .expect_err("comment triggers must fail closed");
        assert!(
            error.to_string().contains("unsupported_event"),
            "unexpected error for {event}: {error}"
        );
    }
    Ok(())
}
