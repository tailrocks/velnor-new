//! Plan closure: freshness gate, publish upload, anchor, legacy path.
use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_workflow_renderer::{
    ACQUIRE_NAME, CHECK_GENERATED_NAME, FRESHNESS_OUTDIR, MATRIX_REPORT_UPLOAD_NAME,
    PUBLISH_PLAN_NAME, RUN_KEY_EXPR, RenderError, SETUP_MISE_NAME, checkout_step, freshness_step,
    matrix_report_upload_step, plan_step, publish_plan_step, render_workflow_ir,
};

use super::impl_renderer_fixtures::*;

#[test]
fn strict_plan_job_gets_full_prelude_in_order() -> Result<(), RenderError> {
    let plan = job(
        "velnor-plan",
        "Velnor Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            acquire_fixture()?,
            plan_step(),
        ],
    );
    let text = strict(&fixture_ir(vec![plan]), &fixture_ctx())?;
    assert_eq!(
        step_names(&text, "velnor-plan"),
        [
            "Checkout",
            "Restore Mise tools",
            SETUP_MISE_NAME,
            ACQUIRE_NAME,
            CHECK_GENERATED_NAME,
            "Write request",
            "Plan",
            PUBLISH_PLAN_NAME,
            "Save Mise tools",
        ]
    );
    Ok(())
}

#[test]
fn strict_rejects_anchorless_plan() -> Result<(), RenderError> {
    let anchorless = job(
        "velnor-plan",
        "Velnor Plan",
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
        "velnor-plan",
        "Velnor Plan",
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
    let step = freshness_step(STAGED, FRESHNESS_OUTDIR)?;
    assert_eq!(step.name, CHECK_GENERATED_NAME);
    let velnor_actions_contract::StepKind::Shell { run, .. } = &step.kind else {
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
            freshness_step(bad, FRESHNESS_OUTDIR).is_err(),
            "binary: {bad}"
        );
    }
    assert!(freshness_step(STAGED, ".github").is_err());
    Ok(())
}

#[test]
fn publish_plan_upload_shape_exact() -> Result<(), RenderError> {
    let step = publish_plan_step()?;
    assert_eq!(step.name, PUBLISH_PLAN_NAME);
    let velnor_actions_contract::StepKind::Action { uses, with } = &step.kind else {
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
    let velnor_actions_contract::StepKind::Action { uses, with } = &step.kind else {
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
fn uploads_carry_if_always_and_downloads_do_not() -> Result<(), RenderError> {
    let plan = job(
        "velnor-plan",
        "Velnor Plan",
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
    let download = velnor_actions_workflow_renderer::steps::download_artifact_step(
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
