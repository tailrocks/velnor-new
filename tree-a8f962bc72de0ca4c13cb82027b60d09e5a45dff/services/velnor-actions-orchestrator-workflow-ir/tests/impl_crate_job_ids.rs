//! Crate-job identity: runnable grouping plus owning-job binding.

use velnor_actions_contract_planning::ProposedTask;
use velnor_actions_orchestrator_workflow_ir::crate_job_ids::job_id_for_member;
use velnor_actions_orchestrator_workflow_ir::crate_jobs::is_runnable;
use velnor_actions_rust::{TaskGroup, TaskKind, propose_task};
use velnor_actions_rust_core::{CompileDriver, NextestProfile, TestRunner};
use velnor_actions_workflow_jobs::context::PLAN_JOB_ID;

/// Runnable fixture proposal for one package/kind pair.
fn group(package: &str, kind: TaskKind) -> ProposedTask {
    let key = if package == "demo" { "root" } else { package };
    let group = TaskGroup {
        task_id: format!("stack/rust/{key}/{}/default", kind.as_str()),
        package_id: format!("{package} 0.1.0"),
        package_name: package.to_owned(),
        manifest_key: key.to_owned(),
        kind,
        configuration: "default".to_owned(),
        features: Vec::new(),
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: CompileDriver::Cargo,
        test_runner: TestRunner::CargoTest,
        nextest_profile: NextestProfile::Default,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        run_ignored: None,
    };
    let task = propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    task
}

#[test]
fn same_package_tasks_share_one_rust_job_id() {
    let clippy = group("demo", TaskKind::Clippy);
    let test = group("demo", TaskKind::Test);
    assert!(is_runnable(&clippy));
    assert!(is_runnable(&test));
    let tasks = vec![clippy, test];
    let first = job_id_for_member(&tasks, &tasks[0]).expect("first binds");
    let second = job_id_for_member(&tasks, &tasks[1]).expect("second binds");
    assert_eq!(first, second);
    assert!(first.starts_with("rust-"), "rust namespace: {first}");
    let outsider = group("other", TaskKind::Clippy);
    assert_eq!(job_id_for_member(&tasks, &outsider), None);
}

#[test]
fn mise_checks_bind_check_id_and_leave_emission() {
    let mut check = group("demo", TaskKind::Test);
    check.stack_id = "mise".to_owned();
    assert!(!is_runnable(&check));
    assert_eq!(
        job_id_for_member(&[check.clone()], &check),
        Some(format!("check-{}", check.identity.unit_id))
    );
}

#[test]
fn targetless_tasks_are_not_runnable() {
    let mut task = group("demo", TaskKind::Test);
    task.no_targets = true;
    assert!(!is_runnable(&task));
}

#[test]
fn unknown_rust_member_binds_nothing() {
    let task = group("demo", TaskKind::Test);
    assert!(is_runnable(&task));
    assert_eq!(job_id_for_member(&[], &task), None);
}

#[test]
fn package_less_tasks_belong_to_plan_job() {
    let mut workspace = group("demo", TaskKind::Fmt);
    workspace.identity.unit_id.clear();
    assert!(!is_runnable(&workspace));
    assert_eq!(
        job_id_for_member(&[workspace.clone()], &workspace).as_deref(),
        Some(PLAN_JOB_ID)
    );
}
