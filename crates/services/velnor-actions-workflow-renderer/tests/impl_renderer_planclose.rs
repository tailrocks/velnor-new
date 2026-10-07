//! Plan closure: freshness gate, publish upload, anchor, legacy path.
use velnor_actions_contract_config::WorkflowPolicy;
use velnor_actions_workflow_renderer::{
    CHECK_GENERATED_NAME, DOWNLOAD_PLAN_NAME, FRESHNESS_OUTDIR, PUBLISH_PLAN_NAME,
    download_plan_step, freshness_step, publish_plan_step, render_workflow_ir,
};
use velnor_actions_workflow_steps::{
    ACQUIRE_NAME, CRATE_REPORT_UPLOAD_NAME, MATRIX_REPORT_UPLOAD_NAME, MERGE_OPERATION,
    RUN_KEY_EXPR, RenderError, SETUP_MISE_NAME, checkout_step, crate_job_report_upload_step,
    matrix_report_upload_step, merge_step, plan_step, write_request_step,
};

use super::impl_renderer_fixtures::*;

#[test]
fn strict_plan_job_gets_full_prelude_in_order() -> Result<(), RenderError> {
    let plan = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            acquire_fixture()?,
            plan_step(),
        ],
    );
    let text = strict(&fixture_ir(vec![plan]), &fixture_ctx())?;
    assert_eq!(
        step_names(&text, "plan"),
        [
            "Checkout",
            "Restore Velnor tool seed",
            SETUP_MISE_NAME,
            ACQUIRE_NAME,
            CHECK_GENERATED_NAME,
            "Write request",
            "Plan",
            PUBLISH_PLAN_NAME,
            "Save Mise tools",
        ]
    );
    assert!(
        !text.contains("Restore Mise tools"),
        "P08: restores stay built-in:\n{text}"
    );
    Ok(())
}

#[test]
fn strict_rejects_anchorless_plan() -> Result<(), RenderError> {
    let anchorless = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, acquire_fixture()?],
    );
    assert!(
        strict(&fixture_ir(vec![anchorless]), &fixture_ctx())
            .is_err_and(|err| format!("{err:?}").contains("plan_job_without_plan_step")),
        "anchorless plan must fail closed"
    );
    Ok(())
}

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

#[test]
fn final_job_gets_download_before_write_request() -> Result<(), RenderError> {
    let (id, mut final_job) = job(
        "required",
        "Required",
        vec!["plan".to_owned()],
        vec![
            acquire_fixture()?,
            write_request_step(MERGE_OPERATION)?,
            merge_step(),
        ],
    );
    final_job.condition = Some("always()".to_owned());
    let plan = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            acquire_fixture()?,
            plan_step(),
        ],
    );
    let text = strict(&fixture_ir(vec![plan, (id, final_job)]), &fixture_ctx())?;
    let names = step_names(&text, "required");
    let count = names
        .iter()
        .filter(|name| name.as_str() == DOWNLOAD_PLAN_NAME)
        .count();
    assert_eq!(count, 1, "exactly one download: {names:?}");
    let download_at = names.iter().position(|name| name == DOWNLOAD_PLAN_NAME);
    let write_at = names.iter().position(|name| name == "Write request");
    let merge_at = names.iter().position(|name| name == "Merge reports");
    assert!(
        download_at.is_some_and(|at| Some(at) < write_at && Some(at) < merge_at)
            && write_at.is_some_and(|at| Some(at) < merge_at),
        "download precedes merge request: {names:?}"
    );
    let at = text.find(DOWNLOAD_PLAN_NAME).expect("download step");
    let window = snip(&text, at, 500);
    assert!(
        !window.contains("if: always()"),
        "job-level always governs:\n{window}"
    );
    assert!(window.contains("velnor-plan-"), "artifact name:\n{window}");
    Ok(())
}

#[test]
fn uploads_carry_if_always_and_downloads_do_not() -> Result<(), RenderError> {
    let plan = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            acquire_fixture()?,
            plan_step(),
        ],
    );
    let text = strict(&fixture_ir(vec![plan]), &fixture_ctx())?;
    let publish_at = text.find(PUBLISH_PLAN_NAME).expect("publish step");
    let window = snip(&text, publish_at, 400);
    assert!(window.contains("if: always()"), "publish if:\n{window}");
    let download = velnor_actions_workflow_steps::steps::download_artifact_step(
        "some-artifact",
        "${{ runner.temp }}/x",
    )?;
    let polling = job(
        "velnor-poll",
        "Poll",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, download],
    );
    let text = strict(&fixture_ir(vec![polling]), &fixture_ctx())?;
    let at = text.find("Download candidate").expect("download step");
    let window = snip(&text, at, 300);
    assert!(!window.contains("if: always()"), "download if:\n{window}");
    Ok(())
}
