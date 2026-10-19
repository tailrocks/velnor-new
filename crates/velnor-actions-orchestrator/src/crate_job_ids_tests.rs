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
fn all_obligations_of_a_crate_share_one_job() {
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
    assert_eq!(groups.len(), 1);
    let mut observed: Vec<_> = groups
        .values()
        .flatten()
        .map(|task| &task.task_id)
        .collect();
    observed.sort();
    let mut expected: Vec<_> = tasks.iter().map(|task| &task.task_id).collect();
    expected.sort();
    assert_eq!(observed, expected);
    for task in &tasks {
        assert_eq!(
            job_id_for_member(&tasks, task).expect("owner"),
            Some("rust-demo".to_owned())
        );
    }
}

#[test]
fn identity_collision_assignment_is_shared_by_planner_and_renderer() {
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
            &expected[&(key.package_id.clone(), key.configuration.clone())]
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
fn each_configuration_owns_its_own_job() {
    let first = task("demo", TaskKind::Clippy);
    let mut second = first.clone();
    second.configuration = "host".to_owned();
    second.task_id = "stack/rust/demo/clippy/host".to_owned();
    let tasks = vec![first, second];
    let assigned = assign_group_ids(&group_runnable(&tasks).expect("groups"));
    assert_eq!(
        job_id_for_member(&tasks, &tasks[0]).expect("default"),
        Some("rust-demo".to_owned())
    );
    assert_eq!(
        job_id_for_member(&tasks, &tasks[1]).expect("host"),
        Some("rust-demo-host".to_owned())
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
fn unknown_stack_fails_without_losing_tasks() {
    let mut unknown = task("demo", TaskKind::Clippy);
    unknown.stack_id = "foreign".to_owned();
    assert!(group_runnable(&[unknown.clone()]).is_err());
    assert!(job_id_for_member(&[unknown.clone()], &unknown).is_err());
}

#[test]
fn collocated_tofu_and_rust_keep_separate_jobs() {
    let mut tofu = task("root", TaskKind::Clippy);
    tofu.stack_id = Stack::Tofu.id().to_owned();
    tofu.task_kind = "validate".to_owned();
    let rust = task("root", TaskKind::Clippy);
    let tasks = vec![tofu, rust];
    let groups = group_runnable(&tasks).expect("mixed source groups");
    assert_eq!(groups.len(), 2);
    assert!(groups.keys().any(|key| key.stack == Stack::Tofu));
    assert!(groups.keys().any(|key| key.stack == Stack::Rust));
    assert_eq!(
        job_id_for_member(&tasks, &tasks[0]).expect("tofu"),
        Some("tofu-root".to_owned())
    );
    assert_eq!(
        job_id_for_member(&tasks, &tasks[1]).expect("rust"),
        Some("rust-root".to_owned())
    );
}

#[test]
fn outsider_package_has_no_owner() {
    let tasks = vec![task("demo", TaskKind::Clippy)];
    assert_eq!(
        job_id_for_member(&tasks, &task("other", TaskKind::Doc)).expect("outsider"),
        None
    );
}
