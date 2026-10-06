use velnor_actions_mise::ArchivePlan;
use velnor_actions_rust::{
    CompileDriver, NextestProfile, ProfileSource, RustExecutionProfile, TaskGroup, TaskKind,
    TestRunner,
};

use super::plan_shard_archive;

/// Minimal group with driver, target, and package selectors.
fn group(driver: CompileDriver, target: &str) -> TaskGroup {
    TaskGroup {
        task_id: "stack/rust/root/nextest/default".to_owned(),
        package_id: "demo".to_owned(),
        package_name: "demo".to_owned(),
        manifest_key: "root".to_owned(),
        kind: TaskKind::Nextest,
        configuration: "default".to_owned(),
        features: Vec::new(),
        target: target.to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: driver,
        test_runner: TestRunner::CargoNextest,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        run_ignored: None,
        nextest_profile: NextestProfile::Default,
    }
}

/// Detected Cargo/Nextest profile selecting the default Nextest profile.
fn profile() -> RustExecutionProfile {
    RustExecutionProfile {
        compile_driver: CompileDriver::Cargo,
        test_runner: TestRunner::CargoNextest,
        evidence: Vec::new(),
        driver_source: ProfileSource::Detected,
        runner_source: ProfileSource::Detected,
        nextest_profile: NextestProfile::Default,
        nextest_config: None,
        run_ignored: None,
    }
}

#[test]
fn archive_planning_errors_instead_of_skipping() {
    let profile = profile();
    let mut archives = ArchivePlan::new();
    let err = plan_shard_archive(
        &group(CompileDriver::Cargo, "${{ x }}"),
        &profile,
        &mut archives,
    )
    .expect_err("bad target fails");
    assert!(err.to_string().contains("archive_plan"), "{err}");
    assert!(archives.is_empty());
    plan_shard_archive(
        &group(CompileDriver::Cargo, "host"),
        &profile,
        &mut archives,
    )
    .expect("planned");
    assert_eq!(archives.len(), 1);
    // Re-planning the same configuration dedupes without error.
    plan_shard_archive(
        &group(CompileDriver::Cargo, "host"),
        &profile,
        &mut archives,
    )
    .expect("dedupe");
    assert_eq!(archives.len(), 1);
}
