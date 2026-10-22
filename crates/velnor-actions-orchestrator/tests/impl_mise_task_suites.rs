//! Generated workflow behavior for credential-free Mise task suites.

use std::fs;

use velnor_actions_orchestrator::{GenerateOptions, generate, prepare, render_staged_tree};

use crate::impl_common::{TestResult, make_repo};
use crate::impl_schema2_routing::required_file;

fn assert_push_workflow(workflow: &str) {
    assert!(workflow.contains("push:"));
    assert!(workflow.contains("branches:"));
    assert!(workflow.contains("- testmain"));
    assert!(!workflow.contains("schedule:"));
    assert!(
        workflow.contains("github.event_name == 'push' && github.ref == 'refs/heads/testmain'")
    );
    assert!(workflow.contains("ref: ${{ github.sha }}"));
    assert!(workflow.contains("fetch-depth"), "{workflow}");
    assert!(workflow.contains("persist-credentials"), "{workflow}");
    assert!(workflow.contains("CARGO_BUILD_JOBS"), "{workflow}");
    assert!(workflow.contains("NEXTEST_TEST_THREADS"), "{workflow}");
    assert!(!workflow.contains("concurrency:"));
    assert!(workflow.contains(
        "velnor-mise-task-push-head-ledger-${{ github.run_id }}-attempt-${{ github.run_attempt }}"
    ));
    assert!(workflow.contains("if-no-files-found: error"));
    assert!(!workflow.contains("workflow_dispatch:"));
    assert!(!workflow.contains("pull_request:"));
    assert!(!workflow.contains("merge_group:"));
}

fn assert_scheduled_workflow(workflow: &str) {
    assert!(workflow.contains("schedule:"));
    assert!(workflow.contains("cron: 0 3 * * *"));
    assert!(!workflow.contains("push:"));
    assert!(
        workflow
            .contains("github.event_name == 'schedule' && (github.event.schedule == '0 3 * * *')")
    );
    assert!(workflow.contains("ref: ${{ github.sha }}"));
    assert!(workflow.contains("fetch-depth"), "{workflow}");
    assert!(workflow.contains("persist-credentials"), "{workflow}");
    assert!(!workflow.contains("concurrency:"));
    assert!(workflow.contains(
        "velnor-mise-task-schedule-snapshot-snapshot-${{ github.run_id }}-attempt-${{ github.run_attempt }}"
    ));
    assert!(workflow.contains("if-no-files-found: error"));
}

#[test]
fn task_suites_emit_event_scoped_full_sha_workflows_with_attempt_artifacts() -> TestResult {
    let repo = make_repo(
        r#"
schema = 1
[workflow]
name = "CI"
default_branch = "testmain"

[[workflow.task_suites]]
id = "push-head"
mise_task = "record-push"
source = { mise_config = "mise.toml", working_directory = "." }
runner = "linux-x64"
timeout_minutes = 5
workflow = "evidence"
events = ["push"]

[[workflow.task_suites.artifacts]]
name = "ledger"
paths = ["target/evidence/push-ledger.jsonl"]
when = "success"
retention_days = 45

[[workflow.task_suites]]
id = "schedule-snapshot"
mise_task = "collect-snapshot"
source = { mise_config = "mise.toml", working_directory = "." }
runner = "linux-x64"
timeout_minutes = 15
workflow = "scheduled-evidence"
events = ["schedule"]
schedule = ["0 3 * * *"]

[[workflow.task_suites.artifacts]]
name = "snapshot"
paths = ["target/evidence/snapshot.json"]
when = "success"
retention_days = 45
"#,
    )?;
    fs::write(
        repo.path().join("mise.toml"),
        r#"
min_version = "2026.10.7"

[tasks.record-push]
run = "echo record-push"

[tasks.collect-snapshot]
run = "echo collect-snapshot"
"#,
    )?;

    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let workflow = required_file(&tree, ".github/workflows/evidence.yml")?;
    let scheduled_workflow = required_file(&tree, ".github/workflows/scheduled-evidence.yml")?;
    assert_push_workflow(workflow);
    assert_scheduled_workflow(scheduled_workflow);
    let ci = required_file(&tree, ".github/workflows/ci.yml")?;
    assert!(!ci.contains("mise-task-push-head"));
    assert!(!ci.contains("mise-task-schedule-snapshot"));
    Ok(())
}

#[test]
fn removing_a_task_suite_retires_its_generated_workflow() -> TestResult {
    let config = r#"
schema = 1
[workflow]
name = "CI"
default_branch = "testmain"

[[workflow.task_suites]]
id = "evidence"
mise_task = "collect-evidence"
source = { mise_config = "mise.toml", working_directory = "." }
runner = "linux-x64"
timeout_minutes = 5
workflow = "evidence"
events = ["push"]
"#;
    let repo = make_repo(config)?;
    let root = repo.path();
    fs::write(
        root.join("mise.toml"),
        "[tasks.collect-evidence]\nrun = \"echo evidence\"\n",
    )?;

    let first = prepare(root)?;
    generate(&first, &GenerateOptions::default())?;
    let workflow_path = root.join(".github/workflows/evidence.yml");
    assert!(workflow_path.is_file());

    fs::write(
        root.join(".velnor/config.toml"),
        crate::impl_common::config_with_branch(),
    )?;
    let without_suite = prepare(root)?;
    assert!(
        render_staged_tree(&without_suite)?
            .get(".github/workflows/evidence.yml")
            .is_none()
    );
    generate(&without_suite, &GenerateOptions::default())?;
    assert!(
        !workflow_path.exists(),
        "removed declaration retires owned workflow"
    );
    Ok(())
}
