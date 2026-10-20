//! Nextest payload argv shape cases.
use velnor_actions_contract::ContractError;
use velnor_actions_rust::tasks::{
    TaskGroup, TaskKind, cargo_payload_argv, cargo_payload_with_profile,
};
use velnor_actions_rust::{CompileDriver, NextestProfile, TestRunner};

fn group(kind: TaskKind) -> TaskGroup {
    TaskGroup {
        all_features: false,
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

#[test]
fn nextest_shard_suffix_emits_hash_partition_flag() -> Result<(), ContractError> {
    let mut sharded = group(TaskKind::Nextest);
    sharded.task_id = "stack/rust/root/nextest/default/shard-2-of-4".to_owned();
    sharded.test_runner = TestRunner::CargoNextest;
    sharded.nextest_profile = NextestProfile::Ci;
    assert_eq!(
        profiled(&sharded)?,
        [
            "nextest",
            "run",
            "--profile",
            "ci",
            "--locked",
            "--offline",
            "--manifest-path",
            "Cargo.toml",
            "--package",
            "demo",
            "--no-tests",
            "warn",
            "--partition",
            "hash:2/4",
        ]
    );

    let mut single_shard = group(TaskKind::Nextest);
    single_shard.task_id = "stack/rust/root/nextest/default/shard-1-of-1".to_owned();
    single_shard.test_runner = TestRunner::CargoNextest;
    single_shard.nextest_profile = NextestProfile::Ci;
    assert_eq!(
        profiled(&single_shard)?,
        [
            "nextest",
            "run",
            "--profile",
            "ci",
            "--locked",
            "--offline",
            "--manifest-path",
            "Cargo.toml",
            "--package",
            "demo",
            "--no-tests",
            "fail",
        ]
    );

    let mut unsharded = group(TaskKind::Nextest);
    unsharded.task_id = "stack/rust/root/nextest/default".to_owned();
    unsharded.test_runner = TestRunner::CargoNextest;
    unsharded.nextest_profile = NextestProfile::Ci;
    let argv = profiled(&unsharded)?;
    assert!(
        !argv.iter().any(|arg| arg == "--partition"),
        "unsharded nextest task must not emit --partition: {argv:?}"
    );
    Ok(())
}
