//! Workflow path/ref forms accepted from the Actions run API.

use super::workflow_path_matches;

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
