use std::fs;

use velnor_actions_contract::config::{NextestTestSelection, NextestTestSelectionMode};
use velnor_actions_contract::{StepKind, WorkflowPolicy};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_rust::{
    CompileDriver, NextestProfile, TaskGroup, TaskKind, TestRunner, propose_nextest_selection,
    propose_task,
};

use super::*;

const BASE_ID: &str = "stack/rust/root/nextest/default";

fn task_group(kind: TaskKind) -> TaskGroup {
    TaskGroup {
        task_id: format!("stack/rust/root/{}/default", kind.as_str()),
        package_id: "demo 0.1.0".to_owned(),
        package_name: "demo".to_owned(),
        manifest_key: "root".to_owned(),
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
        test_runner: TestRunner::CargoNextest,
        nextest_profile: NextestProfile::Ci,
        run_ignored: None,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
    }
}

fn selection(ids: &[&str]) -> NextestTestSelection {
    NextestTestSelection {
        package: "demo".to_owned(),
        binary: "control_states".to_owned(),
        mode: NextestTestSelectionMode::IgnoredOnly,
        test_ids: ids.iter().map(|id| (*id).to_owned()).collect(),
    }
}

fn selected_task(selection: &NextestTestSelection, shard: Option<u32>) -> ProposedTask {
    let group = task_group(TaskKind::Nextest);
    let mut task = propose_nextest_selection(&group, selection).expect("valid selector");
    if let Some(index) = shard {
        task.task_id = format!("{BASE_ID}/shard-{index}-of-2");
    }
    task.validate().expect("valid nextest task");
    task
}

fn discovery(tasks: Vec<ProposedTask>) -> Discovery {
    crate::crate_jobs::crate_jobs_tests::discovery(tasks)
}

fn source_root() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("temporary workspace");
    fs::create_dir_all(root.path().join("src")).expect("source directory");
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("manifest");
    fs::write(root.path().join("Cargo.lock"), "version = 4\n").expect("lockfile");
    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn value() -> u8 { 1 }\n",
    )
    .expect("Rust source");
    fs::create_dir_all(root.path().join(".config")).expect("Nextest config directory");
    fs::write(
        root.path().join(".config/nextest.toml"),
        "[profile.ci]\nfail-fast = false\n",
    )
    .expect("Nextest config");
    root
}

#[test]
fn sharded_selection_uses_one_archive_and_keeps_sequential_reported_obligations() {
    let root = source_root();
    let allowlist = selection(&["ignored_alpha", "ignored_beta"]);
    let tasks = vec![
        selected_task(&allowlist, Some(1)),
        selected_task(&allowlist, Some(2)),
    ];
    let built = super::build_crate_jobs_inner(
        Some(root.path()),
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &discovery(tasks),
        &[allowlist],
        &ToolCatalog::pinned(),
        &[],
        None,
        2,
    )
    .expect("archive jobs");

    let producer_id = assert_archive_producer(&built.jobs);
    assert_archive_consumer(&built.jobs, &producer_id);
}

fn assert_archive_producer(jobs: &[(String, Job)]) -> String {
    let producers: Vec<_> = jobs
        .iter()
        .filter(|(id, _)| id.starts_with("nextest-archive-"))
        .collect();
    assert_eq!(producers.len(), 1, "same build identity is archived once");
    let (producer_id, producer) = producers[0];
    assert_eq!(producer.needs, vec![PLAN_JOB_ID.to_owned()]);
    let upload = producer
        .steps
        .iter()
        .find(|step| step.name == "Upload Nextest archive")
        .expect("archive upload");
    assert!(matches!(&upload.kind, StepKind::Action { with, .. }
        if with.get("path").map(String::as_str) == Some(ARCHIVE_PATH)));
    let create = producer
        .steps
        .iter()
        .find(|step| step.name == "Create Nextest archive")
        .expect("archive command");
    assert!(matches!(&create.kind, StepKind::Shell { run, .. }
        if run.iter().any(|arg| arg == "--manifest-path")
            && run.iter().any(|arg| arg == "Cargo.toml")));
    producer_id.clone()
}

fn assert_archive_consumer(jobs: &[(String, Job)], producer_id: &str) {
    let consumer = jobs
        .iter()
        .find(|(id, _)| id == "rust-demo")
        .map(|(_, job)| job)
        .expect("package consumer");
    assert_eq!(
        consumer.needs,
        vec![PLAN_JOB_ID.to_owned(), producer_id.to_owned()]
    );
    let downloads: Vec<_> = consumer
        .steps
        .iter()
        .filter(|step| step.name == "Download Nextest archive")
        .collect();
    assert_eq!(
        downloads.len(),
        1,
        "download once before the shard sequence"
    );
    let names: Vec<_> = consumer
        .steps
        .iter()
        .map(|step| step.name.as_str())
        .collect();
    let download = names
        .iter()
        .position(|name| *name == "Download Nextest archive")
        .expect("download step");
    let shard_one = names
        .iter()
        .position(|name| *name == "Unit and integration tests (shard 1 of 2)")
        .expect("first shard step");
    let shard_two = names
        .iter()
        .position(|name| *name == "Unit and integration tests (shard 2 of 2)")
        .expect("second shard step");
    assert!(download < shard_one && shard_one < shard_two, "{names:?}");
    for shard in [shard_one, shard_two] {
        assert!(
            matches!(&consumer.steps[shard].kind, StepKind::Shell { run, .. }
            if run.iter().any(|arg| arg.contains("test(=ignored_alpha) | test(=ignored_beta)")))
        );
    }
}

#[test]
fn selector_ids_do_not_change_archive_identity_or_direct_task_vectors() {
    let root = source_root();
    let first = selection(&["ignored_alpha"]);
    let second = selection(&["ignored_beta"]);
    let first_task = selected_task(&first, Some(1));
    let second_task = selected_task(&second, Some(1));
    let catalog = ToolCatalog::pinned();
    let first_plan = super::build_archive_plan(
        root.path(),
        "ubuntu-26.04",
        &discovery(vec![first_task]),
        &[first],
        &catalog,
        &[],
    )
    .expect("first plan");
    let second_plan = super::build_archive_plan(
        root.path(),
        "ubuntu-26.04",
        &discovery(vec![second_task]),
        &[second],
        &catalog,
        &[],
    )
    .expect("second plan");
    assert_eq!(
        first_plan.producers.keys().collect::<Vec<_>>(),
        second_plan.producers.keys().collect::<Vec<_>>(),
        "archive identity excludes runtime test IDs"
    );

    let build = propose_task(&task_group(TaskKind::Build)).expect("Build proposal");
    let unsharded = propose_task(&task_group(TaskKind::Nextest)).expect("Nextest proposal");
    for task in [build, unsharded] {
        let input = discovery(vec![task]);
        let direct = crate::crate_jobs::build_crate_jobs(
            "ubuntu-26.04",
            WorkflowPolicy::ConsumerV1,
            &input,
            &catalog,
            &[],
            None,
            2,
        )
        .expect("direct job");
        let archived = super::build_crate_jobs_inner(
            Some(root.path()),
            "ubuntu-26.04",
            WorkflowPolicy::ConsumerV1,
            &input,
            &[],
            &catalog,
            &[],
            None,
            2,
        )
        .expect("workflow job");
        assert_eq!(direct.jobs.len(), archived.jobs.len());
        assert_eq!(
            serde_json::to_value(&direct.jobs[0].1).expect("direct JSON"),
            serde_json::to_value(&archived.jobs[0].1).expect("workflow JSON"),
            "Build and unsharded Nextest output remain unchanged"
        );
        assert!(
            archived
                .jobs
                .iter()
                .all(|(id, _)| !id.starts_with("nextest-archive-"))
        );
    }
}

#[test]
fn archive_selector_rejects_empty_and_mismatched_filters() {
    let empty = selection(&[]);
    assert!(empty.validate(".velnor/config.toml", "selector").is_err());
    let selected = selection(&["ignored_alpha"]);
    let task = selected_task(&selected, Some(1));
    let other = selection(&["ignored_beta"]);
    assert!(super::selection_for(&task, &[other]).is_err());
}
