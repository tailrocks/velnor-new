use super::*;

#[test]
fn tofu_tasks_derive_the_tofu_envelope() {
    use velnor_actions_tofu_core::TofuTaskKind;
    let dir = TempDir::create("tofu-ext").expect("tempdir");
    std::fs::write(dir.path.join(".terraform.lock.hcl"), "lock").expect("lockfile");
    let task = tofu_proposal(TofuTaskKind::Validate);
    let discovery = empty_discovery();
    let snapshot = ExecutionSnapshot::build(&discovery);
    let bundle = crate::internal_plan::identities::extension_bundle_with_snapshot(
        &snapshot,
        &discovery,
        &task,
        Some(&dir.path),
        None,
    );
    let (envelope, eligible) = extension_for_task(
        &task,
        &dir.path,
        &bundle,
        &mut velnor_actions_tofu_core::FileCache::new(),
    )
    .expect("derives");
    assert_eq!(envelope.schema, TOFU_EXTENSION_SCHEMA);
    assert!(
        !eligible,
        "T23: validate reuse is OFF despite a known lockfile"
    );
    assert!(velnor_actions_contract_release::validate_tofu_extension(&envelope).is_ok());
}

#[test]
fn tofu_drift_fails_the_bridge_closed() {
    use velnor_actions_tofu_core::TofuTaskKind;
    let dir = TempDir::create("tofu-drift").expect("tempdir");
    let mut task = tofu_proposal(TofuTaskKind::InitForValidate);
    task.identity.compile_driver = "cargo".to_owned();
    let discovery = empty_discovery();
    let snapshot = ExecutionSnapshot::build(&discovery);
    let bundle = crate::internal_plan::identities::extension_bundle_with_snapshot(
        &snapshot,
        &discovery,
        &task,
        Some(&dir.path),
        None,
    );
    let err = extension_for_task(
        &task,
        &dir.path,
        &bundle,
        &mut velnor_actions_tofu_core::FileCache::new(),
    )
    .expect_err("drift fails");
    assert!(err.to_string().contains("unknown_driver:cargo"), "{err}");
}

#[test]
fn rust_tasks_keep_the_rust_envelope() {
    use velnor_actions_rust::{TaskGroup, TaskKind};
    use velnor_actions_rust_core::{CompileDriver, NextestProfile, TestRunner};
    let group = TaskGroup {
        task_id: "stack/rust/root/clippy/default".to_owned(),
        package_id: "demo".to_owned(),
        package_name: "demo".to_owned(),
        manifest_key: "root".to_owned(),
        kind: TaskKind::Clippy,
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
    let dir = TempDir::create("rust-ext").expect("tempdir");
    let discovery = empty_discovery();
    let snapshot = ExecutionSnapshot::build(&discovery);
    let bundle = crate::internal_plan::identities::extension_bundle_with_snapshot(
        &snapshot,
        &discovery,
        &task,
        Some(&dir.path),
        None,
    );
    let (envelope, eligible) = extension_for_task(
        &task,
        &dir.path,
        &bundle,
        &mut velnor_actions_tofu_core::FileCache::new(),
    )
    .expect("derives");
    assert_eq!(envelope.schema, RUST_EXTENSION_SCHEMA);
    assert!(eligible, "no build script is reuse-eligible");
}
