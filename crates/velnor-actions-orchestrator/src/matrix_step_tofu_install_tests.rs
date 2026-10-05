//! Tofu-install classification tests: suites that spawn `tofu`.
//!
//! Declared via `#[path]` from `matrix_step.rs` under `cfg(test)`.
//! Covers typed tool ownership and rendered install behavior.

use super::*;
use crate::clippy_groups::ClippyMemoryPlan;
use crate::discover::Discovery;
use velnor_actions_contract::{Job, ProposedTask, WorkflowPolicy};
use velnor_actions_rust::{CompileDriver, NextestProfile, TaskGroup, TaskKind, TestRunner};

/// Runnable fixture proposal for one package (test kind, no gates).
fn group(package: &str) -> ProposedTask {
    let key = if package == "demo" { "root" } else { package };
    let group = TaskGroup {
        task_id: format!("stack/rust/{key}/{}/default", TaskKind::Test.as_str()),
        package_id: format!("{package} 0.1.0"),
        package_name: package.to_owned(),
        manifest_key: key.to_owned(),
        kind: TaskKind::Test,
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
    let task = velnor_actions_rust::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    task
}

/// Discovery shell carrying only task proposals.
fn discovery(groups: Vec<ProposedTask>) -> Discovery {
    Discovery {
        mise_checks: Vec::new(),
        statuses: Vec::new(),
        workspaces: Vec::new(),
        proposals: groups,
        feature_fallbacks: Vec::new(),
        tool_checks: Vec::new(),
        clippy_memory: ClippyMemoryPlan {
            groups: Vec::new(),
            barriers: 0,
        },
        recommendations: Vec::new(),
        consumer_manifest_json: None,
        consumer_manifest_stand_in: false,
        skipped_non_utf8: false,
        tofu_note: None,
        tofu_units: Vec::new(),
    }
}

/// `Prepare pinned tools` argv of one built job.
fn prepare_run(job: &Job) -> Vec<String> {
    use velnor_actions_mise::PREPARE_PINNED_TOOLS_STEP;
    let step = job
        .steps
        .iter()
        .find(|step| step.name == PREPARE_PINNED_TOOLS_STEP)
        .expect("prepare step");
    let velnor_actions_contract::StepKind::Shell { run, .. } = &step.kind else {
        panic!("prepare must be a shell step");
    };
    run.clone()
}

#[test]
fn tofu_install_follows_executed_suite_per_policy() {
    // Only the mise suite spawns real tofu (`tofu_exec` plus
    // `run_bounded`), so only its job adds opentofu beyond obligations.
    for package in [
        "velnor-actions-orchestrator",
        "velnor-actions-cli",
        "velnor-actions-contract",
        "velnor-actions-mise",
        "velnor-actions-rust",
        "velnor-actions-tofu",
        "velnor-actions-workflow-renderer",
        "velnor-actions-actionlint",
        "demo",
    ] {
        assert!(
            !crate_needs_tofu_install(WorkflowPolicy::ConsumerV1, suite_for_package(package)),
            "consumer suites cannot reach our tofu_exec ctor: {package} installs no opentofu"
        );
    }
    assert!(
        crate_needs_tofu_install(
            WorkflowPolicy::VelnorRepositoryV1,
            suite_for_package("velnor-actions-mise")
        ),
        "mise spawns real tofu and must install opentofu"
    );
    for package in [
        "velnor-actions-orchestrator",
        "velnor-actions-cli",
        "velnor-actions-contract",
        "velnor-actions-rust",
        "velnor-actions-tofu",
        "velnor-actions-workflow-renderer",
        "velnor-actions-actionlint",
        "demo",
    ] {
        assert!(
            !crate_needs_tofu_install(
                WorkflowPolicy::VelnorRepositoryV1,
                suite_for_package(package)
            ),
            "{package} never spawns tofu and must not install opentofu"
        );
    }
}

#[test]
fn mise_crate_job_prepare_installs_opentofu() {
    use velnor_actions_mise::PinnedTool;
    let catalog = ToolCatalog::pinned();
    let opentofu = catalog.tool_spec(PinnedTool::Opentofu);
    assert_eq!(
        opentofu, "opentofu@1.13.1",
        "render pin names the catalog spec"
    );
    let found = crate::crate_jobs::build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::VelnorRepositoryV1,
        &discovery(vec![
            group("velnor-actions-mise"),
            group("velnor-actions-contract"),
        ]),
        &catalog,
        &[],
        None,
        2,
    )
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 2);
    for (id, job) in &found.jobs {
        let run = prepare_run(job);
        let spawning = id == "rust-velnor-actions-mise";
        assert_eq!(
            run.contains(&opentofu),
            spawning,
            "{id} opentofu membership follows its executed suite: {run:?}"
        );
    }
}
