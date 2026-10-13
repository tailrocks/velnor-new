//! Plan closure (strict): prelude order, anchor gate, download/upload shape.
use velnor_actions_workflow_jobs::{CHECK_GENERATED_NAME, DOWNLOAD_PLAN_NAME, PUBLISH_PLAN_NAME};
use velnor_actions_workflow_steps::{
    ACQUIRE_NAME, MERGE_OPERATION, RenderError, SETUP_MISE_NAME, checkout_step, merge_step,
    plan_step, write_request_step,
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
