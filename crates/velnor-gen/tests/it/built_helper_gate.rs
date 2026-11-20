// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The `built` output of the test step is used only by a job whose pinned helper
//! records it (`Job::records_build`), never because a job lacks a build step.

use velnor_model::{BaseExpiry, CargoCommand, FilterRef, HelperCommand, StepKind, When};
use velnor_render::rendered_workflow;

use crate::facts::TestResult;
use crate::rules::plan;

fn save_condition(records_build: bool) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let mut plan = plan(vec![
        StepKind::Helper(HelperCommand::Restored(BaseExpiry::Never)),
        StepKind::Cargo(CargoCommand::NextestRun {
            filter: FilterRef::All,
            ci_profile: false,
        }),
        StepKind::Helper(HelperCommand::PlanFull),
    ])?;
    plan.jobs[0].records_build = records_build;
    plan.jobs[0].steps[2].when = When::SaveBase;
    let rendered = rendered_workflow(&plan);
    Ok(rendered.jobs[0].steps[2].condition.clone())
}

#[test]
fn a_job_without_a_build_step_on_a_helper_that_does_not_record_it_never_waits_for_built()
-> TestResult {
    let condition = save_condition(false)?.ok_or("no condition")?;
    assert!(condition.contains("success()"), "{condition}");
    assert!(!condition.contains("built"), "{condition}");
    Ok(())
}

#[test]
fn a_job_on_a_helper_that_records_the_build_waits_for_it() -> TestResult {
    let condition = save_condition(true)?.ok_or("no condition")?;
    assert!(condition.contains("outputs.built == 'true'"), "{condition}");
    Ok(())
}
