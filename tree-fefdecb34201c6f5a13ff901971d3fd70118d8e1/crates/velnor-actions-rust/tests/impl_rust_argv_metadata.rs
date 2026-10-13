//! Cargo payload metadata and strict token parsing cases.
use super::{group, profiled, text};
use velnor_actions_contract::ContractError;
use velnor_actions_rust::tasks::{
    cargo_payload_argv, cargo_payload_with_profile, entry_metadata, evidence_id,
};
use velnor_actions_rust::{CompileDriver, Evidence, EvidenceStrength, NextestProfile, TestRunner};

#[test]
fn entry_metadata_carries_driver_runner_and_evidence() {
    let group_case = group(super::TaskKind::Test);
    let sightings = vec![
        Evidence {
            path: "scripts/test.sh".to_owned(),
            line: 2,
            command_or_setting: "cargo test --package a".to_owned(),
            strength: EvidenceStrength::Durable,
        },
        Evidence {
            path: "mise.toml".to_owned(),
            line: 4,
            command_or_setting: "mr_boxington = true".to_owned(),
            strength: EvidenceStrength::Durable,
        },
    ];
    let metadata = entry_metadata(&group_case, &sightings);
    assert_eq!(metadata.compile_driver, CompileDriver::Cargo);
    assert_eq!(metadata.test_runner, TestRunner::CargoTest);
    assert_eq!(metadata.evidence_ids.len(), 2);
    assert!(metadata.evidence_ids[0].starts_with("scripts/test.sh:2:"));
    assert!(metadata.evidence_ids[1].starts_with("mise.toml:4:"));
    assert_eq!(metadata.evidence_ids[0], evidence_id(&sightings[0]));
    assert_eq!(
        entry_metadata(&group_case, &sightings),
        metadata,
        "entry metadata is deterministic"
    );
}

#[test]
fn nextest_payload_is_pinned_tool_input() -> Result<(), ContractError> {
    let mut configured = group(super::TaskKind::Nextest);
    configured.nextest_profile = NextestProfile::Ci;
    assert_eq!(
        profiled(&configured)?,
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
    let mut custom = group(super::TaskKind::Nextest);
    custom.manifest_key = "crates/demo".to_owned();
    assert!(
        profiled(&custom)?
            .windows(2)
            .any(|w| w == ["--manifest-path", "crates/demo/Cargo.toml"])
    );
    assert!(
        profiled(&custom)?
            .windows(2)
            .any(|w| w == ["--profile", "default"])
    );
    Ok(())
}

#[test]
fn unknown_profile_token_fails_closed() {
    assert_eq!(NextestProfile::parse("ci"), Ok(NextestProfile::Ci));
    assert_eq!(
        NextestProfile::parse("default"),
        Ok(NextestProfile::Default)
    );
    for bad in ["", "nightly", "CI", "ci ", "default\n"] {
        let err = NextestProfile::parse(bad).expect_err("unknown profile");
        assert!(err.to_string().contains("unknown_profile"), "{err}");
    }
}

#[test]
fn driver_runner_tokens_parse_strictly() {
    assert_eq!(CompileDriver::parse("cargo"), Ok(CompileDriver::Cargo));
    assert_eq!(CompileDriver::parse("mbx"), Ok(CompileDriver::Mbx));
    assert_eq!(TestRunner::parse("cargo_test"), Ok(TestRunner::CargoTest));
    assert_eq!(
        TestRunner::parse("cargo_nextest"),
        Ok(TestRunner::CargoNextest)
    );
    for bad in ["", "bogus", "Cargo", "cargo ", "nextest", "cargo-nextest"] {
        let driver = CompileDriver::parse(bad).expect_err("unknown driver");
        assert!(driver.to_string().contains("unknown_driver"), "{driver}");
        let runner = TestRunner::parse(bad).expect_err("unknown runner");
        assert!(runner.to_string().contains("unknown_runner"), "{runner}");
    }
    for (parsed, spelling) in [
        (CompileDriver::Cargo.as_str(), "cargo"),
        (CompileDriver::Mbx.as_str(), "mbx"),
        (TestRunner::CargoTest.as_str(), "cargo_test"),
        (TestRunner::CargoNextest.as_str(), "cargo_nextest"),
    ] {
        assert_eq!(parsed, spelling);
    }
}

#[test]
fn leading_dash_values_fail_closed() {
    let mut package = group(super::TaskKind::Clippy);
    package.package_name = "-evil".to_owned();
    let err = cargo_payload_argv(&package).expect_err("dash package");
    assert!(err.to_string().contains("leading_dash_package"), "{err}");

    let mut manifest = group(super::TaskKind::Clippy);
    manifest.manifest_key = "-evil".to_owned();
    let err = cargo_payload_argv(&manifest).expect_err("dash manifest");
    assert!(err.to_string().contains("leading_dash_manifest"), "{err}");

    let mut features = group(super::TaskKind::Test);
    features.features = vec!["-evil".to_owned()];
    let err = cargo_payload_argv(&features).expect_err("dash features");
    assert!(err.to_string().contains("leading_dash_features"), "{err}");

    let mut target = group(super::TaskKind::Build);
    target.target = "--help".to_owned();
    let err = cargo_payload_argv(&target).expect_err("dash target");
    assert!(err.to_string().contains("leading_dash_target"), "{err}");

    let err = cargo_payload_with_profile(&target).expect_err("profiled dash target");
    assert!(err.to_string().contains("leading_dash_target"), "{err}");
}

#[test]
fn nextest_payload_carries_run_ignored() -> Result<(), ContractError> {
    let mut g = group(super::TaskKind::Nextest);
    g.test_runner = TestRunner::CargoNextest;
    g.run_ignored = Some("all".to_owned());
    let argv = text(&g)?;
    assert!(
        argv.windows(2).any(|w| w == ["--run-ignored", "all"]),
        "must contain --run-ignored all: {argv:?}"
    );

    let mut g_default = group(super::TaskKind::Nextest);
    g_default.test_runner = TestRunner::CargoNextest;
    g_default.run_ignored = Some("default".to_owned());
    let argv_default = text(&g_default)?;
    assert!(
        !argv_default.iter().any(|arg| arg == "--run-ignored"),
        "default mode must not emit --run-ignored: {argv_default:?}"
    );

    Ok(())
}
