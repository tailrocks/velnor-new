use velnor_actions_orchestrator_run_select::SelectedBaseRun;
use velnor_actions_orchestrator_run_select::select_baseline_artifact;
use velnor_actions_orchestrator_run_select::select_exact_base_run;

/// One push-run listing entry with the fields selection reads.
fn run(id: u64, sha: &str, branch: &str, event: &str, conclusion: &str, attempt: u64) -> String {
    format!(
        "{{\"databaseId\":{id},\"headSha\":\"{sha}\",\"headBranch\":\"{branch}\",\"event\":\"{event}\",\"conclusion\":\"{conclusion}\",\"attempt\":{attempt}}}"
    )
}

/// Only an explicit `"expired": false` selects; absent, null, true,
/// and non-bool markers never do.
#[test]
fn artifact_expiry_must_be_explicitly_false() {
    let listed = serde_json::json!({"artifacts": [
        {"id": 10, "name": "n", "expired": false},
    ]});
    assert_eq!(select_baseline_artifact(&listed.to_string(), "n"), Ok(10));
    for (label, entry) in [
        ("absent", serde_json::json!({"id": 11, "name": "n"})),
        (
            "null",
            serde_json::json!({"id": 12, "name": "n", "expired": null}),
        ),
        (
            "true",
            serde_json::json!({"id": 13, "name": "n", "expired": true}),
        ),
        (
            "string",
            serde_json::json!({"id": 14, "name": "n", "expired": "false"}),
        ),
        (
            "number",
            serde_json::json!({"id": 15, "name": "n", "expired": 0}),
        ),
    ] {
        let listed = serde_json::json!({"artifacts": [entry]});
        assert!(
            select_baseline_artifact(&listed.to_string(), "n").is_err(),
            "{label} expiry must never select"
        );
    }
}

/// The listing is newest-first, so the first match wins.
#[test]
fn newest_matching_run_wins() {
    let text = format!(
        "[{},{}]",
        run(7, "abc", "main", "push", "success", 2),
        run(5, "abc", "main", "push", "success", 1),
    );
    assert_eq!(
        select_exact_base_run(&text, "abc", "main"),
        Ok(SelectedBaseRun {
            run_id: 7,
            attempt: 2,
        })
    );
}

/// A run on another commit never selects.
#[test]
fn wrong_sha_never_selects() {
    let text = format!("[{}]", run(7, "def", "main", "push", "success", 2));
    assert_eq!(
        select_exact_base_run(&text, "abc", "main"),
        Err("baseline_unavailable".to_owned())
    );
}

/// A run on another branch never selects.
#[test]
fn wrong_branch_never_selects() {
    let text = format!("[{}]", run(7, "abc", "side", "push", "success", 2));
    assert_eq!(
        select_exact_base_run(&text, "abc", "main"),
        Err("baseline_unavailable".to_owned())
    );
}

/// Non-push events never select, even when successful.
#[test]
fn non_push_event_never_selects() {
    let text = format!("[{}]", run(7, "abc", "main", "pull_request", "success", 2));
    assert_eq!(
        select_exact_base_run(&text, "abc", "main"),
        Err("baseline_unavailable".to_owned())
    );
}

/// Non-success conclusions never select.
#[test]
fn failed_conclusion_never_selects() {
    let text = format!("[{}]", run(7, "abc", "main", "push", "failure", 2));
    assert_eq!(
        select_exact_base_run(&text, "abc", "main"),
        Err("baseline_unavailable".to_owned())
    );
}

/// Runs without a positive recorded attempt never select.
#[test]
fn missing_attempt_never_selects() {
    let zero = format!("[{}]", run(7, "abc", "main", "push", "success", 0));
    assert_eq!(
        select_exact_base_run(&zero, "abc", "main"),
        Err("baseline_unavailable".to_owned())
    );
    let absent = "[{\"databaseId\":7,\"headSha\":\"abc\",\"headBranch\":\"main\",\"event\":\"push\",\"conclusion\":\"success\"}]";
    assert_eq!(
        select_exact_base_run(absent, "abc", "main"),
        Err("baseline_unavailable".to_owned())
    );
}

/// Malformed listings fail with the unavailable marker.
#[test]
fn malformed_run_listing_is_unavailable() {
    assert_eq!(
        select_exact_base_run("not json", "abc", "main"),
        Err("baseline_unavailable".to_owned())
    );
    assert_eq!(
        select_exact_base_run("[]", "abc", "main"),
        Err("baseline_unavailable".to_owned())
    );
}

/// The `gh api` object shape selects the exact unexpired entry.
#[test]
fn object_shape_selects_exact_entry() {
    let listed = serde_json::json!({"artifacts": [
        {"id": 3, "name": "other", "expired": false},
        {"id": 4, "name": "n", "expired": false},
    ]});
    assert_eq!(select_baseline_artifact(&listed.to_string(), "n"), Ok(4));
}

/// A bare array listing selects too.
#[test]
fn bare_array_shape_selects() {
    let listed = serde_json::json!([
        {"id": 6, "name": "n", "expired": false},
    ]);
    assert_eq!(select_baseline_artifact(&listed.to_string(), "n"), Ok(6));
}

/// Expired exact-name entries never select.
#[test]
fn expired_entry_never_selects() {
    let listed = serde_json::json!({"artifacts": [
        {"id": 8, "name": "n", "expired": true},
    ]});
    assert_eq!(
        select_baseline_artifact(&listed.to_string(), "n"),
        Err("baseline_unavailable".to_owned())
    );
}

/// Shapes without an entry list fail with the unavailable marker.
#[test]
fn entry_list_without_shape_is_unavailable() {
    assert_eq!(
        select_baseline_artifact("{\"artifacts\":{}}", "n"),
        Err("baseline_unavailable".to_owned())
    );
    assert_eq!(
        select_baseline_artifact("not json", "n"),
        Err("baseline_unavailable".to_owned())
    );
}

/// Entries may carry the run id under `databaseId`.
#[test]
fn database_id_fallback_selects() {
    let listed = serde_json::json!({"artifacts": [
        {"databaseId": 9, "name": "n", "expired": false},
    ]});
    assert_eq!(select_baseline_artifact(&listed.to_string(), "n"), Ok(9));
}
