use super::closure::*;
use super::closure_slots::{lock_digest_at_root, nextest_digest_at_root};
use super::identities::*;
use super::snapshot::canonical_digest;
use super::*;
use velnor_actions_contract::digest_b3;
use velnor_actions_contract::validate_digest;
use velnor_actions_contract_planning::ProposedTask;
use velnor_actions_mise::ToolCatalog;
use velnor_actions_rust::{TaskGroup, TaskKind};
use velnor_actions_rust_core::{CompileDriver, NextestProfile, TestRunner};

/// Minimal proposal with kind, task ID, driver, and runner.
fn group(kind: TaskKind, task_id: &str, driver: CompileDriver, runner: TestRunner) -> ProposedTask {
    let group = TaskGroup {
        task_id: task_id.to_owned(),
        package_id: "demo".to_owned(),
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
        compile_driver: driver,
        test_runner: runner,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        run_ignored: None,
        nextest_profile: NextestProfile::Default,
    };
    let task = velnor_actions_rust::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    task
}

mod closure_tests;
mod identities_tests;
mod internal_plan_tests;
mod toolchain;
