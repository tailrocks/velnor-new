//! Workflow path/ref forms accepted from the Actions run API.

use super::{RunResponse, requested_attempt_matches, workflow_path_matches};
use velnor_actions_contract::QualificationRunRef;

#[test]
fn parses_the_github_run_attempt_id_field() {
    // GitHub's run-attempt endpoint returns `id`, not `run_id`.
    let response: RunResponse = serde_json::from_str(
        r#"{
            "id": 37280681115,
            "run_attempt": 1,
            "event": "workflow_dispatch",
            "status": "completed",
            "conclusion": "success",
            "head_branch": "main",
            "head_sha": "0123456789abcdef0123456789abcdef01234567",
            "path": ".github/workflows/ci.yml",
            "head_repository": { "full_name": "tailrocks/velnor-new" }
        }"#,
    )
    .expect("GitHub run-attempt response");

    assert!(requested_attempt_matches(
        &response,
        QualificationRunRef {
            run_id: 37_280_681_115,
            run_attempt: 1,
        }
    ));
    assert!(!requested_attempt_matches(
        &response,
        QualificationRunRef {
            run_id: 37_280_681_116,
            run_attempt: 1,
        }
    ));
    assert!(!requested_attempt_matches(
        &response,
        QualificationRunRef {
            run_id: 37_280_681_115,
            run_attempt: 2,
        }
    ));
}

#[test]
fn accepts_plain_or_default_branch_ci_path_only() {
    let path = velnor_actions_workflow_renderer::render::WORKFLOW_PATH;
    for accepted in [
        path.to_owned(),
        format!("{path}@main"),
        format!("{path}@refs/heads/main"),
    ] {
        assert!(workflow_path_matches(&accepted, "main"), "{accepted}");
    }
    for rejected in [
        ".github/workflows/release.yml".to_owned(),
        format!("{path}@feature/main"),
        format!("{path}@refs/heads/feature"),
    ] {
        assert!(!workflow_path_matches(&rejected, "main"), "{rejected}");
    }
}
