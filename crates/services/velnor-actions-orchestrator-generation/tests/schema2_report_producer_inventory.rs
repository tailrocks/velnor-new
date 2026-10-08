//! Explicit-Both producer inventory reaches the rendered finalized Required graph.

use super::*;

#[test]
fn configured_both_emits_finalized_direct_report_producer_keys() -> TestResult {
    let repo = release_repo(&both_config())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let ci = required_file(&tree, ".github/workflows/ci.yml")?;
    assert_inventory(ci, &["rust-demo__hosted", "rust-demo__local"])
}

#[test]
fn dispatch_override_opts_in_and_hosted_mode_stays_absent() -> TestResult {
    let repo = make_repo(&hosted_schema2())?;
    let prep = prepare(repo.path())?;
    let hosted = render_staged_tree(&prep)?;
    let forced = render_staged_tree_with(&prep, Some(ExecutionMode::Both))?;
    let hosted_ci = required_file(&hosted, ".github/workflows/ci.yml")?;
    let forced_ci = required_file(&forced, ".github/workflows/ci.yml")?;
    assert!(!hosted_ci.contains("VELNOR_TASK_REPORT_PRODUCERS_EXPECTED"));
    assert_inventory(forced_ci, &["rust-demo__hosted", "rust-demo__local"])
}

fn assert_inventory(ci: &str, expected_ids: &[&str]) -> TestResult {
    let required = job_body(ci, "required")?;
    for producer_id in expected_ids {
        let need = format!("      - {producer_id}");
        assert!(
            required.lines().any(|line| line == need),
            "producer {producer_id} must be a direct Required need: {required}"
        );
    }
    let line = required
        .lines()
        .find(|line| line.contains("VELNOR_TASK_REPORT_PRODUCERS_EXPECTED:"))
        .ok_or("missing producer inventory")?;
    let json = serde_json::to_string(expected_ids)?;
    let escaped = json.replace('\\', "\\\\").replace('"', "\\\"");
    let expected = format!("VELNOR_TASK_REPORT_PRODUCERS_EXPECTED: \"{escaped}\"");
    assert!(
        line.contains(&expected),
        "expected {expected:?}, got {line:?}"
    );
    Ok(())
}

#[test]
fn schema1_both_dispatch_preserves_legacy_rendering() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let legacy = render_staged_tree(&prep)?;
    let dispatched = render_staged_tree_with(&prep, Some(ExecutionMode::Both))?;

    assert_eq!(
        dispatched, legacy,
        "schema 1 dispatch must keep legacy output"
    );
    let ci = required_file(&dispatched, ".github/workflows/ci.yml")?;
    assert!(
        !ci.contains("VELNOR_TASK_REPORT_PRODUCERS_EXPECTED"),
        "{ci}"
    );
    Ok(())
}
