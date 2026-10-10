//! Obligation-step contract tests.
//!
//! Declared via `#[path]` from `matrix_step.rs` under `cfg(test)`.

use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
};

use super::*;
use velnor_actions_contract::cachekey::{ToolchainInputs, toolchain_id};
use velnor_actions_contract::workflow::crate_job::task_digest_for_execution;
use velnor_actions_contract::{
    Concurrency, Job, JobTimeout, Permissions, Trigger, WorkflowIr, WorkflowPolicy,
};
use velnor_actions_mise::{PinnedTool, PinnedToolExec, PrepareRustComponents};
use velnor_actions_rust::{
    CompileDriver, NextestProfile, TaskGroup, TaskKind, TestRunner, cargo_payload_env,
};

/// Obligation fixture for step construction.
fn obligation() -> CrateObligation {
    let task_id = "stack/rust/demo/clippy/default".to_owned();
    let matrix_id = velnor_actions_contract::matrix_id_for_task_group("rust", &task_id)
        .expect("matrix identity");
    let matrix_key = velnor_actions_contract::matrix_key_for_id(&matrix_id).expect("matrix key");
    let catalog = ToolCatalog::pinned();
    let run = PinnedToolExec::new(
        vec![PinnedTool::Rust],
        OsStr::new("cargo"),
        ["clippy"].into_iter().map(OsString::from).collect(),
    )
    .expect("pinned task")
    .argv(&catalog)
    .into_iter()
    .map(|word| word.into_string().expect("UTF-8 task argv"))
    .collect::<Vec<_>>();
    let toolchain_inputs = ToolchainInputs {
        tools: catalog.tool_specs(&[PinnedTool::Rust]),
        components: PrepareRustComponents::components(),
        compile_driver: "cargo".to_owned(),
        test_runner: "cargo_test".to_owned(),
    };
    let toolchain = toolchain_id(&toolchain_inputs).expect("toolchain identity");
    CrateObligation {
        task_id: task_id.clone(),
        kind: "clippy".to_owned(),
        step_name: "Clippy".to_owned(),
        gated_by: Vec::new(),
        matrix_key,
        task_digest: task_digest_for_execution(&task_id, &run, &toolchain).expect("task digest"),
        toolchain_inputs,
        run,
    }
}

fn refresh_task_identity(obligation: &mut CrateObligation) {
    let stack = if obligation.task_id.starts_with("stack/tofu/") {
        let catalog = ToolCatalog::pinned();
        obligation.toolchain_inputs = ToolchainInputs {
            tools: catalog.tool_specs(&[PinnedTool::Opentofu]),
            components: Vec::new(),
            compile_driver: "tofu".to_owned(),
            test_runner: "tofu".to_owned(),
        };
        obligation.run = PinnedToolExec::new(
            vec![PinnedTool::Opentofu],
            OsStr::new("tofu"),
            ["-chdir", "dir-", "validate"]
                .into_iter()
                .map(OsString::from)
                .collect(),
        )
        .expect("pinned tofu task")
        .argv(&catalog)
        .into_iter()
        .map(|word| word.into_string().expect("UTF-8 task argv"))
        .collect();
        "tofu"
    } else {
        "rust"
    };
    let matrix_id = velnor_actions_contract::matrix_id_for_task_group(stack, &obligation.task_id)
        .expect("matrix id");
    obligation.matrix_key =
        velnor_actions_contract::matrix_key_for_id(&matrix_id).expect("matrix key");
    let toolchain = toolchain_id(&obligation.toolchain_inputs).expect("toolchain identity");
    obligation.task_digest =
        task_digest_for_execution(&obligation.task_id, &obligation.run, &toolchain)
            .expect("task digest");
}

#[test]
fn identity_env_contract_enforces_in_every_build() {
    let task_id = "stack/rust/demo/clippy/default";
    let identity = obligation_identity_env(
        task_id,
        &format!("b3-{}", "a".repeat(64)),
        "id",
        "key",
        None,
    );
    assert!(check_identity_env_contract(&identity, task_id).is_ok());
    let mut drifted = identity.clone();
    drifted.insert(
        TASK_ID_ENV.to_owned(),
        "stack/rust/demo/test/default".to_owned(),
    );
    let err = check_identity_env_contract(&drifted, task_id).expect_err("drift");
    assert!(
        err.to_string().contains("obligation_identity_mismatch"),
        "{err}"
    );
    let mut missing = identity.clone();
    missing.remove(TASK_ID_ENV);
    assert!(check_identity_env_contract(&missing, task_id).is_err());
    assert_eq!(OBLIGATION_TASK_ID_ENV, TASK_ID_ENV);
    // Log forging: a hostile task id renders as one truncated line.
    let forged = "stack/rust/demo/clippy/default\n::notice::spoofed";
    let err = check_identity_env_contract(&missing, forged).expect_err("forged");
    let text = err.to_string();
    assert!(!text.contains('\n'), "{text}");
    assert!(
        text.contains(
            "obligation_identity_mismatch:stack/rust/demo/clippy/default::notice::spoofed"
        ),
        "{text}"
    );
}

#[test]
fn obligation_step_carries_the_report_lookup_key() {
    let obligation = obligation();
    let step = obligation_step(
        &obligation,
        &ToolCatalog::pinned(),
        &[],
        None,
        env!("CARGO_PKG_VERSION"),
    )
    .expect("step");
    let velnor_actions_contract::StepKind::TaskExecution {
        task_id,
        task_digest,
        argv,
        ..
    } = &step.kind
    else {
        panic!("obligation must be a typed task step");
    };
    assert_eq!(task_id, &obligation.task_id);
    assert_eq!(task_digest, &obligation.task_digest);
    assert_eq!(argv, &obligation.run);
}

#[test]
fn generated_pinned_obligation_renders_through_shared_declared_task_action() {
    let checkout_uses = format!("actions/checkout@{:040x}", 0);
    let task_step = generated_pinned_task_step();
    let ir = obligation_workflow_ir(&checkout_uses, task_step);
    let context = renderer_context(checkout_uses);
    let yaml = velnor_actions_workflow_renderer::render_workflow_ir(
        &ir,
        WorkflowPolicy::ConsumerV1,
        None,
        &context,
    )
    .expect("render generated workflow");
    assert!(
        yaml.contains("uses: ./.github/actions/declared-task-0"),
        "generated obligation should use the shared action:\n{yaml}"
    );
    assert!(
        yaml.contains("argv_10: demo"),
        "task argv preserved:\n{yaml}"
    );
    assert!(
        yaml.contains("task_id: stack/rust/demo/clippy/default"),
        "report identity preserved as input:\n{yaml}"
    );
}

#[test]
fn actual_nextest_producer_selector_passes_task_execution_contract() {
    let catalog = ToolCatalog::pinned();
    let task = velnor_actions_rust::propose_task(&TaskGroup {
        task_id: "stack/rust/demo/nextest/default".to_owned(),
        package_id: String::new(),
        package_name: String::new(),
        manifest_key: "root".to_owned(),
        kind: TaskKind::Nextest,
        configuration: "default".to_owned(),
        features: Vec::new(),
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: CompileDriver::Mbx,
        test_runner: TestRunner::CargoNextest,
        nextest_profile: NextestProfile::Default,
        run_ignored: None,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
    })
    .expect("actual Rust task proposal");
    let run = crate::vectors::task_argv(&task, &catalog).expect("catalog-backed task argv");
    let nextest_spec = catalog.tool_spec(PinnedTool::Nextest);
    assert_eq!(
        nextest_spec,
        "aqua:nextest-rs/nextest/cargo-nextest@0.9.148"
    );
    assert!(run.contains(&nextest_spec), "producer argv: {run:?}");

    let toolchain_inputs = crate::internal_plan::identities::toolchain_inputs_for(&task, &catalog)
        .expect("catalog-backed toolchain identity");
    let toolchain = toolchain_id(&toolchain_inputs).expect("toolchain digest");
    let task_digest =
        task_digest_for_execution(&task.task_id, &run, &toolchain).expect("task digest");
    let matrix_id = velnor_actions_contract::matrix_id_for_task_group("rust", &task.task_id)
        .expect("matrix identity");
    let obligation = CrateObligation {
        task_id: task.task_id.clone(),
        kind: task.task_kind.clone(),
        step_name: "Nextest".to_owned(),
        gated_by: Vec::new(),
        matrix_key: velnor_actions_contract::matrix_key_for_id(&matrix_id).expect("matrix key"),
        task_digest,
        toolchain_inputs,
        run,
    };
    let checkout_uses = format!("actions/checkout@{:040x}", 0);
    let step = obligation_step(&obligation, &catalog, &[], None, env!("CARGO_PKG_VERSION"))
        .expect("typed task step");
    let velnor_actions_contract::StepKind::TaskExecution {
        report_helper_version,
        ..
    } = &step.kind
    else {
        panic!("obligation must remain a typed task step");
    };
    assert_eq!(report_helper_version, env!("CARGO_PKG_VERSION"));
    let ir = obligation_workflow_ir(&checkout_uses, step);
    let context = renderer_context(checkout_uses);
    velnor_actions_workflow_renderer::render_workflow_ir(
        &ir,
        WorkflowPolicy::ConsumerV1,
        None,
        &context,
    )
    .expect("actual producer output passes the TaskExecution contract");
}

#[test]
fn actual_obligation_producer_renders_150_jobs_through_one_typed_action_shape() {
    let checkout_uses = format!("actions/checkout@{:040x}", 0);
    let mut ir = obligation_workflow_ir(&checkout_uses, generated_pinned_task_step());
    ir.jobs.remove("rust-demo");
    let catalog = ToolCatalog::pinned();
    for index in 0..150 {
        let package = format!("crate-{index}");
        let mut task = obligation();
        task.task_id = format!("stack/rust/{package}/test/default");
        task.kind = "test".to_owned();
        task.step_name = "Test".to_owned();
        task.run = PinnedToolExec::new(
            vec![PinnedTool::Rust],
            OsStr::new("cargo"),
            ["test", "-p", package.as_str()]
                .into_iter()
                .map(OsString::from)
                .collect(),
        )
        .expect("pinned Cargo test")
        .argv(&catalog)
        .into_iter()
        .map(|word| word.into_string().expect("UTF-8 task argv"))
        .collect();
        refresh_task_identity(&mut task);
        let step = obligation_step(&task, &catalog, &[], None, env!("CARGO_PKG_VERSION"))
            .expect("producer step");
        ir.jobs.insert(
            format!("rust-crate-{index}"),
            obligation_task_job(&checkout_uses, step),
        );
    }

    let context = renderer_context(checkout_uses);
    let yaml = velnor_actions_workflow_renderer::render_workflow_ir(
        &ir,
        WorkflowPolicy::ConsumerV1,
        None,
        &context,
    )
    .expect("render 150 producer-backed obligations");

    assert_eq!(
        yaml.matches("uses: ./.github/actions/declared-task-0")
            .count(),
        150,
        "each generated obligation remains a job and calls the shared action"
    );
    for index in 0..150 {
        assert!(yaml.contains(&format!("task_id: stack/rust/crate-{index}/test/default")));
        assert!(yaml.contains(&format!("crate-{index}")));
    }
}

#[path = "matrix_step_test_support.rs"]
mod support;
use support::*;

#[path = "matrix_step_tests_b.rs"]
mod reporting_tests;
