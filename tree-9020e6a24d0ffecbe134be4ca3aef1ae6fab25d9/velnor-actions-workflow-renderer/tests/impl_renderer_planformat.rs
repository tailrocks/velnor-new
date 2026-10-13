//! Plan `Format` step: insertion between staging and freshness.
use velnor_actions_workflow_renderer::plan_format::{FORMAT_STEP_NAME, ensure_plan_format};
use velnor_actions_workflow_renderer::{
    CHECK_GENERATED_NAME, RenderError, checkout_step, plan_step,
};

use super::impl_renderer_fixtures::*;

#[test]
fn plan_format_inserts_between_staging_and_plan() -> Result<(), RenderError> {
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
    let mut jobs = fixture_ir(vec![plan]).jobs;
    ensure_plan_format(
        &mut jobs,
        mise_argv("rust@1.98.1", "cargo", &["fmt", "--check"]),
    )?;
    let names: Vec<&str> = jobs["velnor-plan"]
        .steps
        .iter()
        .map(|step| step.name.as_str())
        .collect();
    assert_eq!(
        names,
        ["Checkout", "Acquire Velnor", FORMAT_STEP_NAME, "Plan"]
    );
    let text = strict(&fixture_ir(jobs.into_iter().collect()), &fixture_ctx())?;
    let rendered = step_names(&text, "velnor-plan");
    let format_at = rendered.iter().position(|name| name == FORMAT_STEP_NAME);
    let fresh_at = rendered
        .iter()
        .position(|name| name == CHECK_GENERATED_NAME);
    assert!(
        format_at.is_some_and(|at| Some(at) < fresh_at),
        "format precedes freshness: {rendered:?}"
    );
    Ok(())
}

#[test]
fn plan_format_is_idempotent_and_validates_shape() -> Result<(), RenderError> {
    let plan = job(
        "velnor-plan",
        "Velnor Plan",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, plan_step()],
    );
    let mut jobs = fixture_ir(vec![plan]).jobs;
    let argv = mise_argv("rust@1.98.1", "cargo", &["fmt", "--check"]);
    ensure_plan_format(&mut jobs, argv.clone())?;
    ensure_plan_format(&mut jobs, argv)?;
    let count = jobs["velnor-plan"]
        .steps
        .iter()
        .filter(|step| step.name == FORMAT_STEP_NAME)
        .count();
    assert_eq!(count, 1);
    let fresh = job(
        "velnor-plan",
        "Velnor Plan",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, plan_step()],
    );
    let mut fresh_jobs = fixture_ir(vec![fresh]).jobs;
    assert!(
        ensure_plan_format(&mut fresh_jobs, vec!["cargo".to_owned(), "fmt".to_owned()],)
            .is_err_and(|err| format!("{err:?}").contains("format_without_mise")),
        "bare-cargo format must fail"
    );
    let lonely = job(
        "velnor-task",
        "Velnor Task",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?],
    );
    let mut jobs = fixture_ir(vec![lonely]).jobs;
    ensure_plan_format(&mut jobs, mise_argv("rust@1.98.1", "cargo", &["fmt"]))?;
    Ok(())
}

#[test]
fn plan_format_rejects_non_shell_format() -> Result<(), RenderError> {
    let mut renamed = checkout_step(&checkout_pin())?;
    renamed.name = FORMAT_STEP_NAME.to_owned();
    let plan = job(
        "velnor-plan",
        "Velnor Plan",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, renamed, plan_step()],
    );
    let mut jobs = fixture_ir(vec![plan]).jobs;
    assert!(
        ensure_plan_format(&mut jobs, mise_argv("rust@1.98.1", "cargo", &["fmt"]),)
            .is_err_and(|err| format!("{err:?}").contains("format_step_malformed")),
        "action-shaped format must fail"
    );
    Ok(())
}
