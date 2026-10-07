//! Plan, final, publish, and preflight constructors keep their shape.

use velnor_actions_contract_workflow::StepKind;
use velnor_actions_mise::ToolCatalog;
use velnor_actions_orchestrator_workflow_ir::mbx_preflight::steps_for_catalog;
use velnor_actions_orchestrator_workflow_ir::publish_job::baseline_publish_job;
use velnor_actions_orchestrator_workflow_ir::workflow::CHECKOUT_USES;
use velnor_actions_orchestrator_workflow_ir::workflow::wire_w1::checkout_step;
use velnor_actions_orchestrator_workflow_ir::workflow_jobs::{final_job, plan_job};
use velnor_actions_workflow_jobs::context::{FINAL_CONDITION, FINAL_JOB_ID, PLAN_JOB_ID};

#[test]
fn plan_job_runs_ungated_with_tool_install() {
    let job = plan_job(
        "ubuntu-26.04",
        None,
        &ToolCatalog::pinned(),
        true,
        false,
        false,
        false,
        &[],
    )
    .expect("plan job");
    assert_eq!(job.display_name, "Plan");
    assert_eq!(job.runs_on, "ubuntu-26.04");
    assert!(job.needs.is_empty());
    assert_eq!(job.condition, None);
    let names: Vec<&str> = job.steps.iter().map(|step| step.name.as_str()).collect();
    assert!(names.contains(&"Prepare pinned tools"));
    assert!(names.contains(&"Prepare Rust components"));
}

#[test]
fn final_job_gates_on_plan_crate_jobs_and_lint() {
    let job = final_job(
        "ubuntu-26.04",
        &["rust-demo".to_owned()],
        None,
        &ToolCatalog::pinned(),
    )
    .expect("final job");
    assert_eq!(
        job.needs,
        [
            PLAN_JOB_ID.to_owned(),
            "rust-demo".to_owned(),
            "actionlint".to_owned(),
        ]
    );
    assert_eq!(job.condition.as_deref(), Some(FINAL_CONDITION));
}

#[test]
fn publish_job_needs_final_and_gates_branch_push() {
    let job = baseline_publish_job("ubuntu-26.04", "testmain", None).expect("publish job");
    assert_eq!(job.display_name, "Publish baseline");
    assert_eq!(job.needs, [FINAL_JOB_ID.to_owned()]);
    assert_eq!(
        job.condition.as_deref(),
        Some("github.event_name == 'push' && github.ref == 'refs/heads/testmain'")
    );
    let names: Vec<&str> = job.steps.iter().map(|step| step.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Download plan",
            "Write request",
            "Publish baseline",
            "Upload baseline",
        ]
    );
}

#[test]
fn final_job_without_crates_gates_on_plan_and_lint() {
    let job = final_job("ubuntu-26.04", &[], None, &ToolCatalog::pinned()).expect("final job");
    assert_eq!(job.needs, [PLAN_JOB_ID.to_owned(), "actionlint".to_owned()]);
    assert_eq!(job.condition.as_deref(), Some(FINAL_CONDITION));
}

#[test]
fn checkout_step_uses_canonical_pin() {
    let step = checkout_step().expect("checkout step");
    let StepKind::Action { uses, .. } = &step.kind else {
        panic!("checkout must be an action step");
    };
    assert_eq!(uses, CHECKOUT_USES);
}

#[test]
fn publish_job_prepends_acquire_step() {
    let acquire = checkout_step().expect("acquire step");
    let name = acquire.name.clone();
    let job = baseline_publish_job("ubuntu-26.04", "main", Some(acquire)).expect("publish job");
    assert_eq!(job.steps[0].name, name);
    assert_eq!(job.steps.len(), 5);
}

#[test]
fn preflight_emits_three_distinct_named_steps() {
    let steps = steps_for_catalog(&ToolCatalog::pinned()).expect("preflight");
    let names: Vec<&str> = steps.iter().map(|step| step.name.as_str()).collect();
    assert_eq!(names.len(), 3);
    assert!(names.iter().all(|name| !name.is_empty()));
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 3);
}
