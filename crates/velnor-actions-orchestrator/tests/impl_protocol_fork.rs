//! Fork routing: fork pull-request payloads materialize the Fork event.

use std::fs;

use tempfile::TempDir;
use velnor_actions_orchestrator::write_request_parts;

use crate::impl_common::TestResult;

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
        write_request_parts(&file, "pull_request", &payload, None)?;
        let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file)?)?;
        assert_eq!(value["event"], expected, "fork={fork}");
        assert_eq!(value["base"], base);
        assert_eq!(value["head"], head);
    }
    Ok(())
}
