use tempfile::TempDir;
use velnor_actions_orchestrator::{
    PlanOutputMode, plan_internal, plan_outputs, publish_plan_files,
};

use crate::impl_common::{TestResult, config_with_branch, git, git_line, make_repo};

#[test]
fn split_outputs_reject_a_matrix_copy_that_diverges_from_the_plan() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "plan fixture"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": null,
        "head": head,
        "event": "push",
        "root": root.display().to_string(),
    });
    let response = plan_internal(&request.to_string())?;
    let mut split: serde_json::Value = serde_json::from_str(&response)?;
    assert_eq!(split["matrix"], split["plan"]["matrix"]);
    assert!(
        split["plan"]["matrix"]["include"]
            .as_array()
            .is_some_and(|entries| !entries.is_empty())
    );
    split["matrix"]["include"] = serde_json::json!([]);
    let tampered = split.to_string();

    let output_error = plan_outputs(&tampered, PlanOutputMode::Static)
        .expect_err("split output must reject a different matrix");
    assert!(
        output_error
            .to_string()
            .contains("response_matrix_mismatch")
    );

    let artifacts = TempDir::new()?;
    let publish_error = publish_plan_files(&tampered, artifacts.path())
        .expect_err("artifact publisher must reject a different matrix");
    assert!(
        publish_error
            .to_string()
            .contains("response_matrix_mismatch")
    );
    assert!(!artifacts.path().join("local").exists());
    Ok(())
}
