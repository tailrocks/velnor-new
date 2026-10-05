//! Registered suite admission regressions.

use super::*;

#[test]
fn native_owner_requires_neither_validator_nor_tofu_execution() {
    assert_eq!(
        crate_suite_tools(
            WorkflowPolicy::VelnorRepositoryV1,
            Some("velnor-actions-native")
        )
        .expect("registered native owner"),
        SuiteTools::NONE
    );
}

#[test]
fn runner_cli_suite_requires_no_v1_extra_tools() {
    assert_eq!(
        crate_suite_tools(
            WorkflowPolicy::VelnorRepositoryV1,
            Some("velnor-runner-cli")
        )
        .expect("registered runner CLI suite"),
        SuiteTools::NONE
    );
}

#[test]
fn runner_core_suite_requires_no_v1_extra_tools() {
    assert_eq!(
        crate_suite_tools(
            WorkflowPolicy::VelnorRepositoryV1,
            Some("velnor-runner-core")
        )
        .expect("registered runner core suite"),
        SuiteTools::NONE
    );
}

#[test]
fn runner_github_suite_requires_no_v1_extra_tools() {
    assert_eq!(
        crate_suite_tools(
            WorkflowPolicy::VelnorRepositoryV1,
            Some("velnor-runner-github")
        )
        .expect("registered runner GitHub suite"),
        SuiteTools::NONE
    );
}

#[test]
fn runner_host_suite_requires_no_v1_extra_tools() {
    assert_eq!(
        crate_suite_tools(
            WorkflowPolicy::VelnorRepositoryV1,
            Some("velnor-runner-host")
        )
        .expect("registered runner host suite"),
        SuiteTools::NONE
    );
}

#[test]
fn unregistered_repository_suite_is_rejected() {
    for package in [
        "demo",
        "velnor-actions-unknown",
        "velnor-actions-native-extra",
        "velnor-runner-unknown",
    ] {
        assert!(
            crate_suite_tools(WorkflowPolicy::VelnorRepositoryV1, Some(package))
                .expect_err("suite requires an audit")
                .to_string()
                .contains("unclassified_repository_suite"),
            "{package}"
        );
    }
}

#[test]
fn opaque_consumer_suite_retains_validators() {
    let tools = crate_suite_tools(WorkflowPolicy::ConsumerV1, Some("demo"))
        .expect("consumer suites are opaque");
    assert!(tools.generate_validators);
    assert!(!tools.opentofu);
}

#[test]
fn non_rust_repository_obligation_has_no_compiled_suite() {
    assert_eq!(
        crate_suite_tools(WorkflowPolicy::VelnorRepositoryV1, None)
            .expect("driver selected separately"),
        SuiteTools::NONE
    );
}

fn task(package: &str) -> ProposedTask {
    use velnor_actions_rust::{CompileDriver, NextestProfile, TaskGroup, TaskKind, TestRunner};
    velnor_actions_rust::propose_task(&TaskGroup {
        task_id: format!("stack/rust/{package}/test/default"),
        package_id: format!("{package} 0.1.0"),
        package_name: package.to_owned(),
        manifest_key: package.to_owned(),
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
    })
    .expect("Rust fixture")
}

#[test]
fn mixed_group_uses_rust_owner_after_non_rust_first_task() {
    let mut tofu = task("root");
    tofu.stack_id = Stack::Tofu.id().to_owned();
    let rust = task("velnor-actions-cli");
    let tools = suite_tools_for_tasks(WorkflowPolicy::VelnorRepositoryV1, &[&tofu, &rust])
        .expect("registered Rust owner");
    assert!(tools.generate_validators);
    assert!(!tools.opentofu);
}

#[test]
fn conflicting_rust_owners_are_rejected() {
    let first = task("velnor-actions-cli");
    let second = task("velnor-actions-mise");
    assert!(
        suite_tools_for_tasks(WorkflowPolicy::VelnorRepositoryV1, &[&first, &second])
            .expect_err("ambiguous suite owner")
            .to_string()
            .contains("inconsistent_repository_suite_owner")
    );
}
