//! `test = false` target handling: no test commands for test-less crates.
use velnor_actions_rust::tasks::cargo_payload_argv;
use velnor_actions_rust::{DeriveInputs, TaskKind, derive_task_groups, propose_task};
use velnor_actions_rust_core::{
    CompileDriver, NextestProfile, PackageRecord, ProfileSource, RustExecutionProfile,
    TargetRecord, TestRunner,
};

/// Target entry for fixtures.
fn target(kind: &str, name: &str, test: bool, doctest: bool) -> TargetRecord {
    TargetRecord {
        kind: kind.to_owned(),
        name: name.to_owned(),
        test,
        doctest,
        required_features: Vec::new(),
    }
}

/// Fuzz-like package: one bin with `test = false`, no test-bearing target.
fn untestable_bin_package() -> PackageRecord {
    PackageRecord {
        id: "fuzz-id".to_owned(),
        name: "fuzz".to_owned(),
        version: "0.1.0".to_owned(),
        manifest: "crates/fuzz/Cargo.toml".to_owned(),
        external: false,
        in_workspace: true,
        targets: vec![target("bin", "fuzz-bin", false, false)],
        features: Vec::new(),
        has_build_script: false,
    }
}

/// Package mixing a testable lib with an opted-out bin.
fn mixed_package() -> PackageRecord {
    let mut package = untestable_bin_package();
    package.targets = vec![
        target("lib", "fuzz", true, true),
        target("bin", "fuzz-bin", false, false),
    ];
    package
}

/// Profile selecting Cargo and plain `cargo test`.
fn cargo_profile() -> RustExecutionProfile {
    RustExecutionProfile {
        compile_driver: CompileDriver::Cargo,
        test_runner: TestRunner::CargoTest,
        evidence: Vec::new(),
        driver_source: ProfileSource::Detected,
        runner_source: ProfileSource::Detected,
        nextest_profile: NextestProfile::Default,
        nextest_config: None,
        run_ignored: None,
    }
}

/// Profile selecting MBX and Nextest.
fn nextest_profile() -> RustExecutionProfile {
    RustExecutionProfile {
        compile_driver: CompileDriver::Mbx,
        test_runner: TestRunner::CargoNextest,
        evidence: Vec::new(),
        driver_source: ProfileSource::Detected,
        runner_source: ProfileSource::Detected,
        nextest_profile: NextestProfile::Ci,
        nextest_config: Some(".config/nextest.toml".to_owned()),
        run_ignored: None,
    }
}

/// Derivation inputs for `package` under `profile`.
fn inputs<'a>(
    package: &'a PackageRecord,
    profile: &'a RustExecutionProfile,
    features: &'a [String],
) -> DeriveInputs<'a> {
    DeriveInputs {
        package,
        profile,
        configuration: "default",
        features,
        target: "host",
        explicit_fmt: false,
    }
}

#[test]
fn untestable_bin_omits_both_test_runners() {
    let package = untestable_bin_package();
    let features = vec!["default".to_owned()];
    for profile in [cargo_profile(), nextest_profile()] {
        let inputs = inputs(&package, &profile, &features);
        let Ok(groups) = derive_task_groups(&inputs) else {
            panic!("derivation must succeed");
        };
        for group in &groups {
            let runnable = matches!(
                group.kind,
                TaskKind::Clippy | TaskKind::Build | TaskKind::Doc
            );
            assert_eq!(
                group.no_test_targets,
                !runnable,
                "kind {:?} under {}",
                group.kind,
                group.test_runner.as_str()
            );
        }
        let test = groups
            .iter()
            .find(|group| matches!(group.kind, TaskKind::Test | TaskKind::Nextest))
            .expect("one test group");
        assert!(test.target_flags.is_empty());
        assert!(propose_task(test).expect("proposal").no_targets);
    }
}

#[test]
fn nextest_empty_filtered_selection_stays_fatal_when_target_exists() {
    let package = mixed_package();
    let mut profile = nextest_profile();
    profile.run_ignored = Some("only".to_owned());
    let features = vec!["default".to_owned()];
    let groups = derive_task_groups(&inputs(&package, &profile, &features)).expect("derivation");
    let nextest = groups
        .iter()
        .find(|group| group.kind == TaskKind::Nextest)
        .expect("nextest group");
    assert!(
        !nextest.no_test_targets,
        "the lib target remains applicable"
    );
    assert!(!propose_task(nextest).expect("proposal").no_targets);
    let payload = cargo_payload_argv(nextest).expect("payload");
    let args: Vec<String> = payload
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--run-ignored", "only"])
    );
    assert!(args.windows(2).any(|pair| pair == ["--no-tests", "fail"]));
}

#[test]
fn target_flags_skip_test_opt_out_bins() {
    let package = mixed_package();
    let profile = cargo_profile();
    let features = vec!["default".to_owned()];
    let inputs = inputs(&package, &profile, &features);
    let Ok(groups) = derive_task_groups(&inputs) else {
        panic!("derivation must succeed");
    };
    let test = groups
        .iter()
        .find(|group| group.kind == TaskKind::Nextest)
        .expect("one test group");
    assert!(!test.no_test_targets);
    assert_eq!(test.test_runner, TestRunner::CargoNextest);
    assert_eq!(test.target_flags, vec!["--lib"]);
}
