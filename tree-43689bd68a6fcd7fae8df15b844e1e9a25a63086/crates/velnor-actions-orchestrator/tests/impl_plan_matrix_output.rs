use std::fs;

use tempfile::TempDir;
use velnor_actions_contract::canonical_json_str;
use velnor_actions_orchestrator::{
    PlanOutputMode, plan_internal, plan_outputs, plan_outputs_from_staged_admission,
    publish_plan_files,
};

use crate::impl_common::{TestResult, config_with_branch, git, git_line, make_repo};

#[test]
fn outputs_and_budget_use_the_validated_plan_matrix() -> TestResult {
    let response = push_plan_response()?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let expected_matrix = canonical_json_str(&value["plan"]["matrix"])?;

    let outputs = plan_outputs(&response, PlanOutputMode::DynamicMatrix)?;
    assert_eq!(outputs.matrix, expected_matrix);
    let promoted = outputs.promoted_job_outputs(PlanOutputMode::DynamicMatrix);
    let expected_bytes = promoted
        .iter()
        .map(|(name, output)| (name.encode_utf16().count() + output.encode_utf16().count() + 2) * 2)
        .sum::<usize>();
    assert_eq!(outputs.job_outputs_utf16_bytes, expected_bytes);
    Ok(())
}

#[test]
fn forged_matrix_copy_is_rejected_before_outputs_or_artifacts() -> TestResult {
    let response = push_plan_response()?;
    let mut value: serde_json::Value = serde_json::from_str(&response)?;
    assert_eq!(value["matrix"], value["plan"]["matrix"]);
    assert!(
        value["plan"]["matrix"]["include"]
            .as_array()
            .is_some_and(|entries| !entries.is_empty()),
        "fixture must contain a planned matrix entry"
    );
    value["matrix"]["include"] = serde_json::json!([]);
    let forged = value.to_string();

    let output_error = plan_outputs(&forged, PlanOutputMode::Static)
        .expect_err("output generation must reject a detached matrix");
    assert!(
        output_error
            .to_string()
            .contains("response_matrix_mismatch"),
        "{output_error}"
    );

    let artifacts = TempDir::new()?;
    let velnor_dir = artifacts.path().join("velnor");
    let publish_error = publish_plan_files(&forged, &velnor_dir)
        .expect_err("artifact publication must reject a detached matrix");
    assert!(
        publish_error
            .to_string()
            .contains("response_matrix_mismatch"),
        "{publish_error}"
    );
    assert!(!velnor_dir.exists(), "invalid response created output dir");
    Ok(())
}

#[test]
fn staged_admission_reader_validates_response_before_reading_staged_data() -> TestResult {
    let response = push_plan_response()?;
    let mut value: serde_json::Value = serde_json::from_str(&response)?;
    value["matrix"]["include"] = serde_json::json!([]);
    let forged = value.to_string();

    let runner_temp = TempDir::new()?;
    let staged_dir = runner_temp.path().join("velnor");
    fs::create_dir(&staged_dir)?;
    fs::write(
        staged_dir.join("qualification-cache-admission.json"),
        b"unexpected staged admission",
    )?;
    let error =
        plan_outputs_from_staged_admission(&forged, PlanOutputMode::Static, runner_temp.path())
            .expect_err("staged admission must not bypass response validation");
    assert!(
        error.to_string().contains("response_matrix_mismatch"),
        "{error}"
    );
    Ok(())
}

#[test]
fn invalid_embedded_plan_id_is_rejected_before_artifact_creation() -> TestResult {
    let response = push_plan_response()?;
    let mut value: serde_json::Value = serde_json::from_str(&response)?;
    value["plan"]["plan_id"] = serde_json::json!("forged-plan-id");
    let invalid_plan = value.to_string();
    let artifacts = TempDir::new()?;
    let velnor_dir = artifacts.path().join("velnor");

    let output_error = plan_outputs(&invalid_plan, PlanOutputMode::Static)
        .expect_err("invalid plan must reject output generation");
    assert!(output_error.to_string().contains("plan_mismatch"));
    let publish_error = publish_plan_files(&invalid_plan, &velnor_dir)
        .expect_err("invalid plan must reject artifact publication");
    assert!(publish_error.to_string().contains("plan_mismatch"));
    assert!(!velnor_dir.exists(), "invalid plan created output dir");
    Ok(())
}

#[test]
fn unsupported_response_schema_is_rejected_before_file_creation() -> TestResult {
    let response = push_plan_response()?;
    let mut value: serde_json::Value = serde_json::from_str(&response)?;
    value["schema"] = serde_json::json!(2);
    let unsupported = value.to_string();
    let artifacts = TempDir::new()?;
    let velnor_dir = artifacts.path().join("velnor");

    let error = publish_plan_files(&unsupported, &velnor_dir)
        .expect_err("unsupported schema must not create artifacts");
    assert!(error.to_string().contains("unsupported_schema:2"));
    assert!(!velnor_dir.exists());
    Ok(())
}

fn push_plan_response() -> Result<String, Box<dyn std::error::Error>> {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "matrix authority fixture"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": null,
        "head": head,
        "event": "push",
        "root": root.display().to_string(),
    });
    Ok(plan_internal(&request.to_string())?)
}

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
