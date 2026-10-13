//! Resolved Nextest profile emission into payload argv.

use velnor_actions_rust::tasks::{TaskGroup, TaskKind, cargo_payload_with_profile};
use velnor_actions_rust_core::{
    CompileDriver, NextestConfigInput, NextestProfile, ProfileInputs, TestRunner, detect_profile,
};

/// Nextest config input declaring `[profile.ci]` at `line`.
fn ci_config(path: &str, line: u32) -> NextestConfigInput {
    NextestConfigInput {
        path: path.to_owned(),
        profiles: vec!["ci".to_owned()],
        ci_line: Some(line),
    }
}

/// Nextest config input without `[profile.ci]`.
fn default_config(path: &str) -> NextestConfigInput {
    NextestConfigInput {
        path: path.to_owned(),
        profiles: vec!["linux".to_owned()],
        ci_line: None,
    }
}

fn profile_group(kind: TaskKind, runner: TestRunner) -> TaskGroup {
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
        compile_driver: CompileDriver::Cargo,
        test_runner: runner,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        run_ignored: None,
        nextest_profile: NextestProfile::Default,
    }
}

fn payload_text(argv: &[std::ffi::OsString]) -> Vec<String> {
    argv.iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn resolved_profile_reaches_emission() {
    let inputs = ProfileInputs {
        nextest_configs: vec![ci_config(".config/nextest.toml", 4)],
        ..ProfileInputs::default()
    };
    let outcome = detect_profile(&inputs).expect("ci selection");
    let ci = outcome.profile.nextest_profile;
    let inputs = ProfileInputs {
        nextest_configs: vec![default_config(".config/nextest.toml")],
        ..ProfileInputs::default()
    };
    let outcome = detect_profile(&inputs).expect("default selection");
    let default = outcome.profile.nextest_profile;
    assert_eq!((ci.as_str(), default.as_str()), ("ci", "default"));
    for profile in [ci, default] {
        let mut group = profile_group(TaskKind::Nextest, TestRunner::CargoNextest);
        group.nextest_profile = profile;
        let text = payload_text(&cargo_payload_with_profile(&group).expect("valid"));
        assert_eq!(text[..4], ["nextest", "run", "--profile", profile.as_str()]);
    }
    let mut doctests = Vec::new();
    for runner in [TestRunner::CargoTest, TestRunner::CargoNextest] {
        let group = profile_group(TaskKind::Doctest, runner);
        doctests.push(payload_text(
            &cargo_payload_with_profile(&group).expect("valid"),
        ));
    }
    assert_eq!(doctests[0], doctests[1]);
    assert!(doctests[0].contains(&"--doc".to_owned()));
    assert!(!doctests[0].contains(&"--profile".to_owned()));
}

#[test]
fn nextest_profile_names_are_stable() {
    assert_eq!(NextestProfile::Ci.as_str(), "ci");
    assert_eq!(NextestProfile::Default.as_str(), "default");
}
