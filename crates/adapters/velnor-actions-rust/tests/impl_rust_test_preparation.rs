//! Test preparation command matches the selected test execution.
use velnor_actions_contract::ContractError;
use velnor_actions_rust::tasks::{TaskGroup, TaskKind, cargo_payload_with_profile};
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
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: CompileDriver::Mbx,
        test_runner: TestRunner::CargoTest,
        nextest_profile: NextestProfile::Default,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        run_ignored: None,
    }
}

fn text(group: &TaskGroup) -> Result<Vec<String>, ContractError> {
    Ok(cargo_payload_with_profile(group)?
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect())
}

fn cargo_selection(args: &[String]) -> Vec<&str> {
    let mut selection = Vec::new();
    let mut skip_value = false;
    for arg in args.iter().skip(2) {
        if skip_value {
            skip_value = false;
            continue;
        }
        if ["--profile", "--list-type", "--no-tests"].contains(&arg.as_str()) {
            skip_value = true;
        } else {
            selection.push(arg.as_str());
        }
    }
    selection
}

#[test]
fn nextest_preparation_matches_profile_and_cargo_selection() -> Result<(), ContractError> {
    let mut build = group(TaskKind::Build);
    build.test_runner = TestRunner::CargoNextest;
    build.nextest_profile = NextestProfile::Ci;
    build.features = vec!["serde".to_owned()];
    build.target = "x86_64-unknown-linux-gnu".to_owned();
    let prepared = text(&build)?;
    assert_eq!(
        prepared,
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

    let mut run = group(TaskKind::Nextest);
    run.test_runner = TestRunner::CargoNextest;
    run.nextest_profile = NextestProfile::Ci;
    run.features = build.features.clone();
    run.target = build.target.clone();
    let executed = text(&run)?;
    assert_eq!(&prepared[2..4], &executed[2..4]);
    assert_eq!(cargo_selection(&prepared), cargo_selection(&executed));
    Ok(())
}

#[test]
fn cargo_test_preparation_uses_no_run() -> Result<(), ContractError> {
    assert_eq!(
        text(&group(TaskKind::Build))?,
        [
            "test",
            "--no-run",
            "--locked",
            "--offline",
            "--manifest-path",
            "Cargo.toml",
            "--package",
            "demo",
        ]
    );
    Ok(())
}
