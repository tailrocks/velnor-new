//! P06 detection cases: structural wrappers, Nextest configs, selection.
use velnor_actions_rust_core::{
    AMBIGUOUS_DRIVER_CODE, CompileDriver, MiseWrapperInput, NEXTEST_RECOMMENDATION,
    NextestConfigInput, NextestProfile, PERSIST_EVIDENCE, ProfileError, ProfileInputs,
    ProfileSource, SHADOWED_NEXTEST_CONFIG, TestRunner, detect_profile,
};

/// MBX wrapper input from `path:line`, mirroring the Mise adapter output.
fn mbx_wrapper(path: &str, line: u32) -> MiseWrapperInput {
    MiseWrapperInput {
        path: path.to_owned(),
        line,
        command: "mbx".to_owned(),
        shim_mode: Some("1".to_owned()),
    }
}

/// Non-MBX wrapper input from `path:line`.
fn other_wrapper(path: &str, line: u32, command: &str) -> MiseWrapperInput {
    MiseWrapperInput {
        path: path.to_owned(),
        line,
        command: command.to_owned(),
        shim_mode: None,
    }
}

/// Nextest config input declaring `[profile.ci]` at `line`.
pub(crate) fn ci_config(path: &str, line: u32) -> NextestConfigInput {
    NextestConfigInput {
        path: path.to_owned(),
        profiles: vec!["ci".to_owned()],
        ci_line: Some(line),
    }
}

/// Nextest config input without `[profile.ci]`.
pub(crate) fn default_config(path: &str) -> NextestConfigInput {
    NextestConfigInput {
        path: path.to_owned(),
        profiles: vec!["linux".to_owned()],
        ci_line: None,
    }
}

/// Recommendation codes of an outcome, in order.
fn codes(outcome: &velnor_actions_rust_core::ProfileOutcome) -> Vec<&str> {
    outcome
        .recommendations
        .iter()
        .map(|item| item.code.as_str())
        .collect()
}

#[test]
fn wrapper_only_selects_mbx() {
    let inputs = ProfileInputs {
        mise_wrappers: vec![mbx_wrapper("mise.toml", 1)],
        ..ProfileInputs::default()
    };
    let Ok(outcome) = detect_profile(&inputs) else {
        panic!("wrapper must select a profile");
    };
    assert_eq!(outcome.profile.compile_driver, CompileDriver::Mbx);
    assert_eq!(outcome.profile.driver_source, ProfileSource::Detected);
    assert_eq!(outcome.profile.test_runner, TestRunner::CargoNextest);
    assert_eq!(outcome.profile.evidence.len(), 1);
    let sighting = &outcome.profile.evidence[0];
    assert_eq!((sighting.path.as_str(), sighting.line), ("mise.toml", 1));
    assert!(sighting.command_or_setting.contains("cargo wrapper"));
    assert!(sighting.command_or_setting.contains("\"mbx\""));
    assert!(sighting.command_or_setting.contains("MBX_CARGO_SHIM_MODE"));
    assert_eq!(codes(&outcome), vec![NEXTEST_RECOMMENDATION]);
}

#[test]
fn wrapper_non_mbx_is_explicit_cargo() {
    let inputs = ProfileInputs {
        mise_wrappers: vec![other_wrapper("mise.toml", 3, "sccache")],
        ..ProfileInputs::default()
    };
    let Ok(outcome) = detect_profile(&inputs) else {
        panic!("non-MBX wrapper must select a profile");
    };
    assert_eq!(outcome.profile.compile_driver, CompileDriver::Cargo);
    assert_eq!(outcome.profile.evidence.len(), 1);
    assert!(
        outcome.profile.evidence[0]
            .command_or_setting
            .contains("\"sccache\"")
    );
    assert_eq!(codes(&outcome), vec![NEXTEST_RECOMMENDATION]);
}

#[test]
fn nextest_config_selects_ci() {
    let inputs = ProfileInputs {
        nextest_configs: vec![ci_config(".config/nextest.toml", 4)],
        ..ProfileInputs::default()
    };
    let Ok(outcome) = detect_profile(&inputs) else {
        panic!("nextest config must select a profile");
    };
    assert_eq!(outcome.profile.test_runner, TestRunner::CargoNextest);
    assert_eq!(outcome.profile.runner_source, ProfileSource::Detected);
    assert_eq!(outcome.profile.nextest_profile, NextestProfile::Ci);
    assert_eq!(
        outcome.profile.nextest_config.as_deref(),
        Some(".config/nextest.toml")
    );
    assert_eq!(outcome.profile.compile_driver, CompileDriver::Cargo);
    assert_eq!(outcome.profile.evidence.len(), 1);
    let sighting = &outcome.profile.evidence[0];
    assert_eq!(sighting.command_or_setting, "[profile.ci]");
    assert_eq!(sighting.line, 4);
    assert!(outcome.recommendations.is_empty());
}

#[test]
fn nextest_config_without_ci_selects_default() {
    let inputs = ProfileInputs {
        nextest_configs: vec![default_config(".config/nextest.toml")],
        ..ProfileInputs::default()
    };
    let Ok(outcome) = detect_profile(&inputs) else {
        panic!("nextest config must select a profile");
    };
    assert_eq!(outcome.profile.test_runner, TestRunner::CargoNextest);
    assert_eq!(outcome.profile.nextest_profile, NextestProfile::Default);
    assert_eq!(
        outcome.profile.nextest_config.as_deref(),
        Some(".config/nextest.toml")
    );
    assert_eq!(outcome.profile.evidence.len(), 1);
    assert!(
        outcome.profile.evidence[0]
            .command_or_setting
            .contains("default profile")
    );
}

#[test]
fn combined_wrapper_and_nextest() {
    let inputs = ProfileInputs {
        mise_wrappers: vec![mbx_wrapper("mise.toml", 1)],
        nextest_configs: vec![ci_config(".config/nextest.toml", 4)],
        ..ProfileInputs::default()
    };
    let Ok(outcome) = detect_profile(&inputs) else {
        panic!("combined inputs must select a profile");
    };
    assert_eq!(outcome.profile.compile_driver, CompileDriver::Mbx);
    assert_eq!(outcome.profile.test_runner, TestRunner::CargoNextest);
    assert_eq!(outcome.profile.nextest_profile, NextestProfile::Ci);
    assert_eq!(outcome.profile.evidence.len(), 2);
    assert!(outcome.recommendations.is_empty());
    assert!(outcome.findings.is_empty());
}

#[test]
fn declared_axes_are_independent() {
    let driver_only = ProfileInputs {
        declared_driver: Some(CompileDriver::Mbx),
        nextest_configs: vec![ci_config(".config/nextest.toml", 4)],
        ..ProfileInputs::default()
    };
    let Ok(outcome) = detect_profile(&driver_only) else {
        panic!("declared driver must combine with detected runner");
    };
    assert_eq!(outcome.profile.compile_driver, CompileDriver::Mbx);
    assert_eq!(outcome.profile.driver_source, ProfileSource::Declared);
    assert_eq!(outcome.profile.test_runner, TestRunner::CargoNextest);
    assert_eq!(outcome.profile.runner_source, ProfileSource::Detected);

    let runner_only = ProfileInputs {
        declared_runner: Some(TestRunner::CargoNextest),
        mise_wrappers: vec![mbx_wrapper("mise.toml", 1)],
        ..ProfileInputs::default()
    };
    let Ok(outcome) = detect_profile(&runner_only) else {
        panic!("declared runner must combine with detected driver");
    };
    assert_eq!(outcome.profile.compile_driver, CompileDriver::Mbx);
    assert_eq!(outcome.profile.driver_source, ProfileSource::Detected);
    assert_eq!(outcome.profile.test_runner, TestRunner::CargoNextest);
    assert_eq!(outcome.profile.runner_source, ProfileSource::Declared);
}

#[test]
fn no_evidence_defaults_with_profile_fields() {
    let Ok(outcome) = detect_profile(&ProfileInputs::default()) else {
        panic!("empty inputs must fall back to defaults");
    };
    assert_eq!(outcome.profile.compile_driver, CompileDriver::Cargo);
    assert_eq!(outcome.profile.test_runner, TestRunner::CargoNextest);
    assert_eq!(outcome.profile.nextest_profile, NextestProfile::Default);
    assert_eq!(outcome.profile.nextest_config, None);
    assert_eq!(
        codes(&outcome),
        vec![NEXTEST_RECOMMENDATION, PERSIST_EVIDENCE]
    );
}

#[test]
fn nextest_config_normalizes_with_cargo_test_invocation() {
    let inputs = ProfileInputs {
        nextest_configs: vec![ci_config(".config/nextest.toml", 4)],
        executables: vec![velnor_actions_rust_core::EvidenceFile {
            path: "scripts/test.sh",
            content: "cargo test --package a\n",
        }],
        ..ProfileInputs::default()
    };
    let Ok(outcome) = detect_profile(&inputs) else {
        panic!("config plus cargo-test invocation must normalize");
    };
    assert_eq!(outcome.profile.test_runner, TestRunner::CargoNextest);
    assert_eq!(outcome.profile.evidence.len(), 2);
}

#[test]
fn declared_cargo_conflicts_with_wrapper() {
    let inputs = ProfileInputs {
        declared_driver: Some(CompileDriver::Cargo),
        mise_wrappers: vec![mbx_wrapper("mise.toml", 1)],
        ..ProfileInputs::default()
    };
    let Err(ProfileError::ProfileConflict { declared, evidence }) = detect_profile(&inputs) else {
        panic!("declared cargo plus MBX wrapper must fail");
    };
    assert_eq!(declared, "compile_driver = \"cargo\"");
    assert_eq!(evidence.len(), 1);
}

#[test]
fn declared_mbx_conflicts_with_other_wrapper() {
    let inputs = ProfileInputs {
        declared_driver: Some(CompileDriver::Mbx),
        mise_wrappers: vec![other_wrapper("mise.toml", 3, "sccache")],
        ..ProfileInputs::default()
    };
    let Err(ProfileError::ProfileConflict { declared, evidence }) = detect_profile(&inputs) else {
        panic!("declared MBX plus sccache wrapper must fail");
    };
    assert_eq!(declared, "compile_driver = \"mbx\"");
    assert_eq!(evidence.len(), 1);
}

#[test]
fn distinct_wrappers_are_ambiguous() {
    let inputs = ProfileInputs {
        mise_wrappers: vec![
            mbx_wrapper("mise.toml", 1),
            other_wrapper("crates/a/mise.toml", 2, "sccache"),
        ],
        ..ProfileInputs::default()
    };
    let result = detect_profile(&inputs);
    let Err(ProfileError::AmbiguousDriver { evidence }) = &result else {
        panic!("distinct wrapper commands must fail");
    };
    assert_eq!(evidence.len(), 2);
    assert!(evidence.windows(2).all(|pair| pair[0] <= pair[1]));
    let Err(failed) = result else {
        panic!("distinct wrapper commands must fail");
    };
    assert!(failed.to_string().contains(AMBIGUOUS_DRIVER_CODE));
}

#[test]
fn mbx_invocation_against_other_wrapper_is_ambiguous() {
    let inputs = ProfileInputs {
        mise_wrappers: vec![other_wrapper("mise.toml", 3, "sccache")],
        executables: vec![velnor_actions_rust_core::EvidenceFile {
            path: ".mise/tasks/test",
            content: "#!/bin/sh\nmbx test --package a\n",
        }],
        ..ProfileInputs::default()
    };
    let Err(ProfileError::AmbiguousDriver { evidence }) = detect_profile(&inputs) else {
        panic!("MBX invocation plus sccache wrapper must fail");
    };
    assert_eq!(evidence.len(), 2);
}

#[test]
fn agreeing_nested_wrappers_select_one_driver() {
    let inputs = ProfileInputs {
        mise_wrappers: vec![
            mbx_wrapper("mise.toml", 1),
            mbx_wrapper("crates/a/mise.toml", 5),
        ],
        ..ProfileInputs::default()
    };
    let Ok(outcome) = detect_profile(&inputs) else {
        panic!("agreeing wrappers must select a profile");
    };
    assert_eq!(outcome.profile.compile_driver, CompileDriver::Mbx);
    assert_eq!(outcome.profile.evidence.len(), 2);
}

#[test]
fn nearest_nextest_config_wins_with_shadow_notice() {
    let inputs = ProfileInputs {
        nextest_configs: vec![
            default_config("crates/a/.config/nextest.toml"),
            ci_config(".config/nextest.toml", 4),
        ],
        ..ProfileInputs::default()
    };
    let Ok(outcome) = detect_profile(&inputs) else {
        panic!("nested configs must select a profile");
    };
    assert_eq!(outcome.profile.test_runner, TestRunner::CargoNextest);
    assert_eq!(outcome.profile.nextest_profile, NextestProfile::Default);
    assert_eq!(
        outcome.profile.nextest_config.as_deref(),
        Some("crates/a/.config/nextest.toml")
    );
    assert_eq!(codes(&outcome), vec![SHADOWED_NEXTEST_CONFIG]);
    assert!(
        outcome.recommendations[0]
            .message
            .contains(".config/nextest.toml shadowed")
    );
}
