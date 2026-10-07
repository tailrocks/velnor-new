//! Response-splitting pins: sibling paths, merge verdicts, refusal taxonomy.

use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_internal::internal::{
    merge_passed, plan_outputs, publish_final_report, publish_plan_files, response_path_for,
};
use velnor_actions_orchestrator_plan::plan_output_limits::PlanOutputMode;

fn err_text<T: std::fmt::Debug>(result: Result<T, OrchestratorError>) -> String {
    result.expect_err("must fail").to_string()
}

#[test]
fn response_path_derives_sibling() {
    let temp = tempfile::tempdir().expect("tempdir");
    let request = temp.path().join("plan-request.json");
    assert_eq!(
        response_path_for(&request).expect("sibling"),
        temp.path().join("plan-response.json")
    );
    let nested = temp.path().join("a").join("b").join("merge-request.json");
    assert_eq!(
        response_path_for(&nested).expect("sibling"),
        temp.path().join("a").join("b").join("merge-response.json")
    );
}

#[test]
fn bad_request_file_names_rejected() {
    let temp = tempfile::tempdir().expect("tempdir");
    for name in [
        "plan-response.json",
        "-request.json",
        "request.json",
        "plan.json",
    ] {
        let err = err_text(response_path_for(&temp.path().join(name)));
        assert!(
            err.contains("bad_request_file_name"),
            "{name}: unexpected: {err}"
        );
    }
}

#[test]
fn merge_passed_taxonomy() {
    for (status, passed) in [
        ("passed", true),
        ("no_work", true),
        ("failed", false),
        ("cancelled", false),
        ("blocked", false),
        ("not_run", false),
        ("planning_failed", false),
    ] {
        assert_eq!(
            merge_passed(&format!("{{\"status\":\"{status}\"}}")).expect("verdict"),
            passed,
            "{status}"
        );
    }
}

#[test]
fn malformed_responses_rejected() {
    let temp = tempfile::tempdir().expect("tempdir");
    let err = err_text(plan_outputs("{not json", PlanOutputMode::Static));
    assert!(err.contains("malformed_response"), "unexpected: {err}");
    let err = err_text(publish_plan_files("{not json", temp.path()));
    assert!(err.contains("malformed_response"), "unexpected: {err}");
    let err = err_text(publish_final_report("{not json", temp.path()));
    assert!(err.contains("malformed_response"), "unexpected: {err}");
    let err = err_text(merge_passed("{not json"));
    assert!(err.contains("malformed_response"), "unexpected: {err}");
}
