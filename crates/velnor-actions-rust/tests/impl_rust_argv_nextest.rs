//! Nextest payload argv shape cases.
use velnor_actions_contract::ContractError;
use velnor_actions_rust::tasks::{
    TaskGroup, TaskKind, cargo_payload_argv, cargo_payload_with_profile,
};
use velnor_actions_rust::{CompileDriver, NextestProfile, TestRunner};

fn group(kind: TaskKind) -> TaskGroup {
    TaskGroup {
        task_id: "t".to_owned(),
        package_id: "p".to_owned(),
        package_name: "demo".to_owned(),
        manifest_key: "root".to_owned(),
        kind,
        configuration: "default".to_owned(),
        features: vec!["default".to_owned()],
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: vec!["--lib".to_owned()],
        no_test_targets: false,
        package_arg: None,
        compile_driver: CompileDriver::Cargo,
        test_runner: TestRunner::CargoTest,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        run_ignored: None,
        nextest_profile: NextestProfile::Default,
    }
}

fn text(group: &TaskGroup) -> Result<Vec<String>, ContractError> {
    Ok(cargo_payload_argv(group)?
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect())
}

fn profiled(group: &TaskGroup) -> Result<Vec<String>, ContractError> {
    Ok(cargo_payload_with_profile(group)?
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect())
}

#[test]
fn test_build_prepares_nextest_binaries_without_running_tests() -> Result<(), ContractError> {
    let mut build = group(TaskKind::Build);
    build.test_runner = TestRunner::CargoNextest;
    build.nextest_profile = NextestProfile::Ci;
    build.features = vec!["serde".to_owned()];
    build.target = "x86_64-unknown-linux-gnu".to_owned();
    assert_eq!(
        profiled(&build)?,
        [
            "nextest",
            "list",
            "--profile",
            "ci",
            "--list-type",
            "binaries-only",
            "--locked",
            "--offline",
            "--manifest-path",
            "Cargo.toml",
            "--package",
            "demo",
            "--no-default-features",
            "--features",
            "serde",
            "--target",
            "x86_64-unknown-linux-gnu",
        ]
    );
    assert_eq!(text(&group(TaskKind::Build))?[..2], ["test", "--no-run"]);
    Ok(())
}
