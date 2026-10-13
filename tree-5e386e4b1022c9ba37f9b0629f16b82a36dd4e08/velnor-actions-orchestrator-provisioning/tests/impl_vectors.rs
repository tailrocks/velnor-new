//! Tool-vector routing from the pinned catalog.

use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_orchestrator_provisioning::vectors::{deny_argv, machete_argv, task_argv};

/// Minimal proposal with one compile driver.
fn group_with_driver(
    driver: velnor_actions_rust_core::CompileDriver,
) -> velnor_actions_contract_planning::ProposedTask {
    let group = velnor_actions_rust::TaskGroup {
        task_id: "stack/rust/root/clippy/default".to_owned(),
        package_id: String::new(),
        package_name: String::new(),
        manifest_key: "root".to_owned(),
        kind: velnor_actions_rust::TaskKind::Clippy,
        configuration: "default".to_owned(),
        features: Vec::new(),
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: driver,
        test_runner: velnor_actions_rust_core::TestRunner::CargoTest,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        run_ignored: None,
        nextest_profile: velnor_actions_rust_core::NextestProfile::Default,
    };
    let task = velnor_actions_rust::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    task
}

#[test]
fn cargo_driver_routes_through_cargo() {
    let catalog = ToolCatalog::pinned();
    let argv = task_argv(
        &group_with_driver(velnor_actions_rust_core::CompileDriver::Cargo),
        &catalog,
    )
    .expect("argv");
    let at = argv.iter().position(|arg| arg == "--").expect("separator");
    assert_eq!(argv[at + 1], "cargo");
    assert_eq!(&argv[5..at], &[catalog.tool_spec(PinnedTool::Rust)]);
}

#[test]
fn mbx_driver_routes_through_mbx() {
    let catalog = ToolCatalog::pinned();
    let argv = task_argv(
        &group_with_driver(velnor_actions_rust_core::CompileDriver::Mbx),
        &catalog,
    )
    .expect("argv");
    let at = argv.iter().position(|arg| arg == "--").expect("separator");
    assert_eq!(argv[at + 1], "mbx");
}

#[test]
fn unknown_stack_fails_closed() {
    let catalog = ToolCatalog::pinned();
    let mut task = group_with_driver(velnor_actions_rust_core::CompileDriver::Cargo);
    task.stack_id = "bogus".to_owned();
    let err = task_argv(&task, &catalog).expect_err("unknown stack");
    assert!(
        err.to_string().contains("unknown_task_stack:bogus"),
        "{err}"
    );
}

#[test]
fn deny_runs_isolated_over_declared_roots() {
    let deny = deny_argv(&[String::new(), "crates/velnor-runner".to_owned()]).expect("argv");
    assert_eq!(&deny[..2], ["sh", "-c"]);
    assert!(deny[2].contains("cargo deny --locked"), "{}", deny[2]);
    assert!(
        deny[2].contains("$GITHUB_WORKSPACE/crates/velnor-runner/Cargo.toml"),
        "{}",
        deny[2]
    );
}

#[test]
fn deny_rejects_empty_roots() {
    deny_argv(&[]).expect_err("empty roots must fail");
}

#[test]
fn machete_runs_through_pinned_tool() {
    let machete = machete_argv().expect("argv");
    assert_eq!(
        &machete[..5],
        ["mise", "--no-config", "--no-env", "--no-hooks", "exec"]
    );
    assert!(machete.iter().any(|arg| arg == "cargo"), "{machete:?}");
    assert!(machete.iter().any(|arg| arg == "machete"), "{machete:?}");
}
