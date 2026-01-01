//! Plan `Format` step: insertion between staging and freshness.
use std::collections::BTreeMap;

use velnor_actions_contract_workflow::StepRole;
use velnor_actions_workflow_jobs::plan_format::{FORMAT_STEP_NAME, ensure_plan_format};
use velnor_actions_workflow_steps::{RenderError, checkout_step, plan_step};

use super::impl_jobs_fixtures::*;

fn format_env() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "MISE_RUSTUP_HOME".to_owned(),
            "${{ runner.temp }}/velnor/rustup".to_owned(),
        ),
        (
            "MISE_CARGO_HOME".to_owned(),
            "${{ runner.temp }}/velnor/cargo".to_owned(),
        ),
        ("RUSTUP_TOOLCHAIN".to_owned(), "1.98.1".to_owned()),
    ])
}

#[test]
fn plan_format_is_idempotent_and_validates_shape() -> Result<(), RenderError> {
    let plan = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, plan_step()],
    );
    let mut jobs = fixture_ir(vec![plan]).jobs;
    let argv = mise_argv("rust@1.98.1", "cargo", &["fmt", "--check"]);
    ensure_plan_format(&mut jobs, argv.clone(), &format_env())?;
    ensure_plan_format(&mut jobs, argv, &format_env())?;
    let count = jobs["plan"]
        .steps
        .iter()
        .filter(|step| step.name == FORMAT_STEP_NAME)
        .count();
    assert_eq!(count, 1);
    let fresh = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, plan_step()],
    );
    let mut fresh_jobs = fixture_ir(vec![fresh]).jobs;
    assert!(
        ensure_plan_format(
            &mut fresh_jobs,
            vec!["cargo".to_owned(), "fmt".to_owned()],
            &format_env(),
        )
        .is_err_and(|err| format!("{err:?}").contains("format_without_mise")),
        "bare-cargo format must fail"
    );
    let lonely = job(
        "velnor-task",
        "Task",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?],
    );
    let mut jobs = fixture_ir(vec![lonely]).jobs;
    ensure_plan_format(
        &mut jobs,
        mise_argv("rust@1.98.1", "cargo", &["fmt"]),
        &format_env(),
    )?;
    Ok(())
}

#[test]
fn plan_format_rejects_non_shell_format() -> Result<(), RenderError> {
    let mut renamed = checkout_step(&checkout_pin())?;
    renamed.name = "Presentation-only checkout label".to_owned();
    renamed.role = Some(StepRole::PlanFormat);
    let plan = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, renamed, plan_step()],
    );
    let mut jobs = fixture_ir(vec![plan]).jobs;
    assert!(
        ensure_plan_format(
            &mut jobs,
            mise_argv("rust@1.98.1", "cargo", &["fmt"]),
            &format_env(),
        )
        .is_err_and(|err| format!("{err:?}").contains("format_step_malformed")),
        "action-shaped format must fail"
    );
    Ok(())
}
