//! Plan closure: freshness gate, publish upload, anchor, legacy path.
use velnor_actions_contract_config::WorkflowPolicy;
use velnor_actions_contract_workflow::JobOutput;
use velnor_actions_workflow_jobs::{
    CHECK_GENERATED_NAME, DOWNLOAD_PLAN_NAME, FRESHNESS_OUTDIR, PUBLISH_PLAN_NAME,
    download_plan_step, freshness_step, publish_plan_step,
};
use velnor_actions_workflow_renderer::render_workflow_ir;
use velnor_actions_workflow_steps::{
    CRATE_REPORT_UPLOAD_NAME, MATRIX_REPORT_UPLOAD_NAME, RUN_KEY_EXPR, RenderError,
    SETUP_MISE_NAME, checkout_step, crate_job_report_upload_step, matrix_report_upload_step,
    plan_step,
};

use super::impl_renderer_fixtures::*;

#[test]
fn legacy_preserves_unstaged_render_and_closes_plan() -> Result<(), RenderError> {
    let plan = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, plan_step()],
    );
    let text = render_workflow_ir(
        &fixture_ir(vec![plan]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(text.contains(CHECK_GENERATED_NAME), "freshness:\n{text}");
    assert!(text.contains(PUBLISH_PLAN_NAME), "publish:\n{text}");
    assert!(
        !text.contains(SETUP_MISE_NAME),
        "legacy has no pins:\n{text}"
    );
    Ok(())
}

#[test]
fn freshness_step_shape_exact() -> Result<(), RenderError> {
    let step = freshness_step(STAGED, FRESHNESS_OUTDIR, &std::collections::BTreeMap::new())?;
    assert_eq!(step.name, CHECK_GENERATED_NAME);
    let velnor_actions_contract_workflow::StepKind::Shell { run, .. } = &step.kind else {
        panic!("freshness must be a shell step");
    };
    assert_eq!(run[0], "sh");
    assert!(run[2].contains("generate --output-dir"), "{}", run[2]);
    assert!(run[2].contains(FRESHNESS_OUTDIR), "{}", run[2]);
    assert!(run[2].contains("diff -r --brief .github"), "{}", run[2]);
    assert!(
        run[2].contains(&format!("{FRESHNESS_OUTDIR}/.github")),
        "{}",
        run[2]
    );
    for bad in [
        "velnor-actions",
        "$RUNNER_TEMP/velnor/../escape",
        "$RUNNER_TEMP/velnor/bin/x${{ y }}",
        "relative/path",
    ] {
        assert!(
            freshness_step(bad, FRESHNESS_OUTDIR, &std::collections::BTreeMap::new()).is_err(),
            "binary: {bad}"
        );
    }
    assert!(freshness_step(STAGED, ".github", &std::collections::BTreeMap::new()).is_err());
    Ok(())
}

#[test]
fn publish_plan_upload_shape_exact() -> Result<(), RenderError> {
    let step = publish_plan_step()?;
    assert_eq!(step.name, PUBLISH_PLAN_NAME);
    let velnor_actions_contract_workflow::StepKind::Action { uses, with, .. } = &step.kind else {
        panic!("publish must be an action step");
    };
    assert!(uses.starts_with("actions/upload-artifact@"));
    assert!(with["name"].contains("velnor-plan-"));
    assert!(with["name"].contains(RUN_KEY_EXPR));
    assert!(with["path"].contains(RUN_KEY_EXPR));
    assert_eq!(with["if-no-files-found"].as_str(), "error");
    Ok(())
}

#[test]
fn matrix_report_upload_names_derive_from_run_and_leg() -> Result<(), RenderError> {
    let step = matrix_report_upload_step()?;
    assert_eq!(step.name, MATRIX_REPORT_UPLOAD_NAME);
    let velnor_actions_contract_workflow::StepKind::Action { uses, with, .. } = &step.kind else {
        panic!("matrix upload must be an action step");
    };
    assert!(uses.starts_with("actions/upload-artifact@"));
    assert_eq!(
        with["name"].as_str(),
        "velnor-matrix-r${{ github.run_id }}-a${{ github.run_attempt }}-${{ matrix.matrix_key }}",
    );
    assert!(with["path"].contains("matrix.matrix_key"));
    assert_eq!(with["if-no-files-found"].as_str(), "error");
    Ok(())
}

#[test]
fn crate_upload_names_derive_from_run_and_job() -> Result<(), RenderError> {
    let step = crate_job_report_upload_step("crate_foo")?;
    assert_eq!(step.name, CRATE_REPORT_UPLOAD_NAME);
    assert_eq!(step.id, None);
    assert_eq!(
        step.role,
        Some(velnor_actions_contract_workflow::StepRole::MatrixReportUpload)
    );
    let velnor_actions_contract_workflow::StepKind::Action { uses, with, .. } = &step.kind else {
        panic!("crate upload must be an action step");
    };
    assert!(uses.starts_with("actions/upload-artifact@"));
    assert_eq!(
        with["name"].as_str(),
        "velnor-crate-r${{ github.run_id }}-a${{ github.run_attempt }}-crate_foo",
    );
    assert_eq!(
        with["path"].as_str(),
        "${{ runner.temp }}/velnor/r${{ github.run_id }}-a${{ github.run_attempt }}",
    );
    assert_eq!(with["if-no-files-found"].as_str(), "error");
    assert!(crate_job_report_upload_step("").is_err());
    assert!(crate_job_report_upload_step("Has Space").is_err());
    Ok(())
}

#[test]
fn typed_job_output_renders_the_uploaded_report_artifact_id() -> Result<(), RenderError> {
    let plan = minimal_plan_job()?;
    let mut upload = crate_job_report_upload_step("rust-demo")?;
    upload.id = Some(velnor_actions_contract_workflow::StepId::CrateReportUpload);
    upload.role = Some(velnor_actions_contract_workflow::StepRole::CrateReportUpload);
    let (task_id, mut task) = job(
        "rust-demo",
        "Rust / demo",
        vec!["plan".to_owned()],
        vec![upload],
    );
    task.outputs = vec![JobOutput::task_report_artifact_id()];
    let text = render_workflow_ir(
        &fixture_ir(vec![plan, (task_id, task)]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(text.contains("id: crate-report-upload"), "{text}");
    assert!(
        text.contains(
            "task_report_artifact_id: ${{ steps.crate-report-upload.outputs.artifact-id }}"
        ),
        "{text}"
    );
    let timeout_at = text.find("timeout-minutes:").expect("job timeout renders");
    let env_at = text
        .find("    env:")
        .expect("legacy job environment renders");
    let outputs_at = text.find("    outputs:").expect("typed outputs render");
    let steps_at = text.find("    steps:").expect("job steps render");
    assert!(
        timeout_at < env_at && env_at < outputs_at && outputs_at < steps_at,
        "job output placement must preserve established header/env ordering:\n{text}"
    );
    Ok(())
}

#[test]
fn download_plan_shape_mirrors_publish() -> Result<(), RenderError> {
    let download = download_plan_step()?;
    assert_eq!(download.name, DOWNLOAD_PLAN_NAME);
    let velnor_actions_contract_workflow::StepKind::Action { uses, with, .. } = &download.kind
    else {
        panic!("download must be an action step");
    };
    assert!(uses.starts_with("actions/download-artifact@"));
    let publish = publish_plan_step()?;
    let velnor_actions_contract_workflow::StepKind::Action {
        with: published, ..
    } = &publish.kind
    else {
        panic!("publish must be an action step");
    };
    assert_eq!(with["name"], published["name"], "artifact name agrees");
    assert_eq!(with["path"], published["path"], "artifact path agrees");
    Ok(())
}
