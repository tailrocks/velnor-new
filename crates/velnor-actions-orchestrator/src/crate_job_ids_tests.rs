use super::*;
use velnor_actions_rust::{CompileDriver, NextestProfile, TaskGroup, TaskKind, TestRunner};

fn task(package: &str, kind: TaskKind) -> ProposedTask {
    velnor_actions_rust::propose_task(&TaskGroup {
        task_id: format!("stack/rust/{package}/{}/default", kind.as_str()),
        package_id: format!("{package} 0.1.0"),
        package_name: package.to_owned(),
        manifest_key: package.to_owned(),
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
    })
    .expect("proposal")
}

#[test]
fn all_obligations_have_exactly_one_phase_owner() {
    let tasks: Vec<_> = [
        TaskKind::Fmt,
        TaskKind::Clippy,
        TaskKind::Build,
        TaskKind::Nextest,
        TaskKind::Test,
        TaskKind::Doctest,
        TaskKind::Doc,
    ]
    .into_iter()
    .map(|kind| task("demo", kind))
    .collect();
    let groups = group_runnable(&tasks).expect("groups");
    assert_eq!(groups.len(), 3);
    let mut observed: Vec<_> = groups
        .values()
        .flatten()
        .map(|task| &task.task_id)
        .collect();
    observed.sort();
    let mut expected: Vec<_> = tasks.iter().map(|task| &task.task_id).collect();
    expected.sort();
    assert_eq!(observed, expected);
    for (index, suffix) in ["lint", "lint", "tests", "tests", "tests", "tests", "docs"]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            job_id_for_member(&tasks, &tasks[index]).expect("owner"),
            Some(format!("rust-demo-{suffix}"))
        );
    }
}

#[test]
fn identity_collision_assignment_precedes_phase_suffixes() {
    let first = task("demo", TaskKind::Clippy);
    let mut second = task("other", TaskKind::Clippy);
    second.display_name = first.display_name.clone();
    let tasks = vec![first, second];
    let groups = group_runnable(&tasks).expect("groups");
    let assigned = assign_group_ids(&groups);
    let raw = BTreeSet::from([
        (
            tasks[0].identity.unit_id.clone(),
            "demo".to_owned(),
            "default".to_owned(),
        ),
        (
            tasks[1].identity.unit_id.clone(),
            "demo".to_owned(),
            "default".to_owned(),
        ),
    ]);
    let expected = assign_crate_job_ids(&raw, CRATE_JOB_ID_PREFIX);
    for (key, id) in &assigned {
        assert_eq!(
            id,
            &format!(
                "{}-lint",
                expected[&(key.package_id.clone(), key.configuration.clone())]
            )
        );
    }
    assert_eq!(assigned.values().collect::<BTreeSet<_>>().len(), 2);
    let reversed: Vec<_> = tasks.into_iter().rev().collect();
    assert_eq!(
        assigned,
        assign_group_ids(&group_runnable(&reversed).expect("reversed groups"))
    );
}

#[test]
fn configuration_is_not_overloaded_with_phase() {
    let first = task("demo", TaskKind::Clippy);
    let mut second = first.clone();
    second.configuration = "host".to_owned();
    second.task_id = "stack/rust/demo/clippy/host".to_owned();
    let tasks = vec![first, second];
    let assigned = assign_group_ids(&group_runnable(&tasks).expect("groups"));
    assert_eq!(
        job_id_for_member(&tasks, &tasks[0]).expect("default"),
        Some("rust-demo-lint".to_owned())
    );
    assert_eq!(
        job_id_for_member(&tasks, &tasks[1]).expect("host"),
        Some("rust-demo-host-lint".to_owned())
    );
    assert_eq!(assigned.len(), 2);
}

#[test]
fn workspace_no_targets_and_named_check_keep_their_authorities() {
    let mut workspace = task("demo", TaskKind::Fmt);
    workspace.identity.unit_id.clear();
    let mut absent = task("demo", TaskKind::Doctest);
    absent.no_targets = true;
    let mut check = task("check", TaskKind::Clippy);
    check.stack_id = Stack::Mise.id().to_owned();
    check.identity.unit_id = "ffi".to_owned();
    let tasks = vec![workspace, absent, check];
    assert!(group_runnable(&tasks).expect("empty").is_empty());
    assert_eq!(
        job_id_for_member(&tasks, &tasks[0]).expect("workspace"),
        Some(PLAN_JOB_ID.to_owned())
    );
    assert_eq!(
        job_id_for_member(&tasks, &tasks[1]).expect("no targets"),
        None
    );
    assert_eq!(
        job_id_for_member(&tasks, &tasks[2]).expect("check"),
        Some("check-ffi".to_owned())
    );
}

#[test]
fn unknown_rust_kind_and_stack_fail_without_losing_tasks() {
    let mut unknown = task("demo", TaskKind::Clippy);
    unknown.task_kind = "archive".to_owned();
    assert!(group_runnable(&[unknown.clone()]).is_err());
    assert!(job_id_for_member(&[unknown.clone()], &unknown).is_err());
    unknown.stack_id = "foreign".to_owned();
    assert!(group_runnable(&[unknown]).is_err());
}

#[test]
fn tofu_namespace_is_not_partitioned_by_rust_kind() {
    let mut tofu = task("root", TaskKind::Clippy);
    tofu.stack_id = Stack::Tofu.id().to_owned();
    tofu.task_kind = "validate".to_owned();
    let rust = task("root", TaskKind::Clippy);
    let tasks = vec![tofu, rust];
    let groups = group_runnable(&tasks).expect("mixed source groups");
    assert_eq!(groups.len(), 2);
    assert!(groups.keys().any(|key| key.phase.is_none()));
    assert_eq!(
        job_id_for_member(&tasks, &tasks[0]).expect("tofu"),
        Some("tofu-root".to_owned())
    );
    assert_eq!(
        job_id_for_member(&tasks, &tasks[1]).expect("rust"),
        Some("rust-root-lint".to_owned())
    );
}

#[test]
fn outsider_cannot_claim_a_missing_phase() {
    let tasks = vec![task("demo", TaskKind::Clippy)];
    assert_eq!(
        job_id_for_member(&tasks, &task("demo", TaskKind::Doc)).expect("outsider"),
        None
    );
}
