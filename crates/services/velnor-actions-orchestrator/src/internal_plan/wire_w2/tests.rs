//! W2 wiring tests.
//!
//! Declared via `#[path]` from `wire_w2.rs` under `cfg(test)`.

use super::reuse_stages::{ExpectedReuseIdentity, ObservedRestoreMeta};
use super::*;
use velnor_actions_contract::ProposedTask;
use velnor_actions_mise::CachedTaskDescriptor;
use velnor_actions_rust::{TaskGroup, TaskKind};
use velnor_actions_rust_core::{CompileDriver, NextestProfile, TestRunner};

/// Minimal proposal with kind, task ID, and nondeterminism flags.
fn group(kind: TaskKind, task_id: &str) -> ProposedTask {
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
        compile_driver: CompileDriver::Cargo,
        test_runner: TestRunner::CargoNextest,
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

mod wire_w2_tests;
