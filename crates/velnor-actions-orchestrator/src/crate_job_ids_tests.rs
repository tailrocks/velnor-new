use super::*;
use velnor_actions_contract::WorkflowEvent;
use velnor_actions_rust::{CompileDriver, NextestProfile, TaskGroup, TaskKind, TestRunner};

fn task(package: &str, kind: TaskKind, configuration: &str) -> ProposedTask {
    velnor_actions_rust::propose_task(&TaskGroup {
        all_features: false,
        task_id: format!("stack/rust/{package}/{}/{}", kind.as_str(), configuration),
        package_id: format!("{package} 0.1.0"),
        package_name: package.to_owned(),
        manifest_key: package.to_owned(),
        kind,
        configuration: configuration.to_owned(),
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
    })
    .expect("proposal")
}

#[test]
fn every_rust_configuration_and_task_has_one_package_configuration_owner() {
    let tasks: Vec<_> = vec![
        task("demo", TaskKind::Fmt, "default"),
        task("demo", TaskKind::Clippy, "default"),
        task("demo", TaskKind::Test, "host"),
        task("demo", TaskKind::Doc, "host"),
    ];
    let groups = group_runnable(&tasks).expect("groups");
    assert_eq!(groups.len(), 2);
    assert_eq!(
        groups[&CrateGroupKey {
            package_id: "demo 0.1.0".to_owned(),
            configuration: "default".to_owned(),
            stack: Stack::Rust,
        }]
            .len(),
        2
    );
    assert_eq!(
        groups[&CrateGroupKey {
            package_id: "demo 0.1.0".to_owned(),
            configuration: "host".to_owned(),
            stack: Stack::Rust,
        }]
            .len(),
        2
    );
    for (task, expected) in
        tasks
            .iter()
            .zip(["rust-demo", "rust-demo", "rust-demo-host", "rust-demo-host"])
    {
        assert_eq!(
            job_id_for_member(&tasks, task, WorkflowEvent::PullRequest)
                .expect("valid owner lookup"),
            Some(expected.to_owned())
        );
    }
}

#[test]
fn package_name_collisions_have_stable_distinct_ids() {
    let first = task("demo", TaskKind::Clippy, "default");
    let mut second = task("other", TaskKind::Clippy, "default");
    second.display_name = first.display_name.clone();
    let tasks = vec![first, second];
    let ids = assign_group_ids(&group_runnable(&tasks).expect("groups"));
    assert_eq!(ids.values().collect::<BTreeSet<_>>().len(), 2);
    assert_eq!(
        ids,
        assign_group_ids(
            &group_runnable(&tasks.iter().cloned().rev().collect::<Vec<_>>())
                .expect("reversed groups")
        )
    );
}

#[test]
fn workspace_and_named_check_owners_remain_separate() {
    let mut workspace = task("demo", TaskKind::Fmt, "default");
    workspace.identity.unit_id.clear();
    let mut check = task("check", TaskKind::Clippy, "default");
    check.stack_id = Stack::Mise.id().to_owned();
    check.identity.unit_id = "ffi".to_owned();
    let tasks = vec![workspace.clone(), check.clone()];
    assert_eq!(
        job_id_for_member(&tasks, &workspace, WorkflowEvent::PullRequest).expect("workspace"),
        Some(PLAN_JOB_ID.to_owned())
    );
    assert_eq!(
        job_id_for_member(&tasks, &check, WorkflowEvent::PullRequest).expect("check"),
        Some("check-ffi".to_owned())
    );
}

#[test]
fn unknown_stack_or_rust_kind_fails_closed() {
    let mut unknown = task("demo", TaskKind::Clippy, "default");
    unknown.task_kind = "archive".to_owned();
    assert!(group_runnable(&[unknown.clone()]).is_err());
    unknown.stack_id = "foreign".to_owned();
    assert!(group_runnable(&[unknown]).is_err());
}
