//! Cargo payload argv shape cases (moved from the orchestrator).
use velnor_actions_contract::ContractError;
use velnor_actions_rust::tasks::{
    TaskGroup, TaskKind, cargo_payload_argv, cargo_payload_with_profile, entry_metadata,
    evidence_id,
};
use velnor_actions_rust::{CompileDriver, Evidence, EvidenceStrength, NextestProfile, TestRunner};

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

/// Profiled payload argv with the group's resolved profile.
fn profiled(group: &TaskGroup) -> Result<Vec<String>, ContractError> {
    Ok(cargo_payload_with_profile(group)?
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect())
}

#[test]
fn payload_shapes_per_kind() -> Result<(), ContractError> {
    assert_eq!(
        text(&group(TaskKind::Fmt))?[..3],
        ["fmt", "--check", "--manifest-path"]
    );
    let mut workspace = group(TaskKind::Fmt);
    workspace.package_name.clear();
    assert_eq!(
        text(&workspace)?[..4],
        ["fmt", "--all", "--check", "--manifest-path"]
    );
    let clippy = text(&group(TaskKind::Clippy))?;
    assert!(clippy.contains(&"--all-targets".to_owned()) && clippy.contains(&"demo".to_owned()));
    assert!(text(&group(TaskKind::Test))?.contains(&"--lib".to_owned()));
    assert!(text(&group(TaskKind::Doctest))?.contains(&"--doc".to_owned()));
    Ok(())
}

#[test]
fn payload_features_and_target() -> Result<(), ContractError> {
    let mut custom = group(TaskKind::Test);
    custom.features = vec!["serde".to_owned(), "cli".to_owned()];
    custom.target = "x86_64-unknown-linux-gnu".to_owned();
    let argv = text(&custom)?;
    assert!(argv.contains(&"--no-default-features".to_owned()));
    assert!(
        argv.windows(2)
            .any(|w| w == ["--target", "x86_64-unknown-linux-gnu"])
    );
    assert!(!text(&group(TaskKind::Fmt))?.contains(&"--no-default-features".to_owned()));
    Ok(())
}

#[test]
fn clippy_keeps_feature_args_before_separator() -> Result<(), ContractError> {
    for features in [vec!["serde".to_owned(), "cli".to_owned()], Vec::new()] {
        let mut featured = group(TaskKind::Clippy);
        featured.features = features;
        let argv = text(&featured)?;
        let sep = argv
            .iter()
            .position(|arg| arg == "--")
            .expect("clippy separator");
        let no_defaults = argv
            .iter()
            .position(|arg| arg == "--no-default-features")
            .expect("feature flag");
        assert!(no_defaults < sep, "cargo flags before `--`: {argv:?}");
        if let Some(features) = argv.iter().position(|arg| arg == "--features") {
            assert!(features < sep, "cargo flags before `--`: {argv:?}");
        }
        assert_eq!(
            &argv[sep..],
            ["--", "-D", "warnings"],
            "only lint args after `--`: {argv:?}"
        );
    }
    Ok(())
}

#[test]
fn clippy_keeps_target_before_separator() -> Result<(), ContractError> {
    let mut targeted = group(TaskKind::Clippy);
    targeted.target = "x86_64-unknown-linux-gnu".to_owned();
    let argv = text(&targeted)?;
    let sep = argv
        .iter()
        .position(|arg| arg == "--")
        .expect("clippy separator");
    let target = argv
        .iter()
        .position(|arg| arg == "--target")
        .expect("target flag");
    assert!(target < sep, "cargo flags before `--`: {argv:?}");
    assert!(
        argv.windows(2)
            .any(|w| w == ["--target", "x86_64-unknown-linux-gnu"]),
        "target triple intact: {argv:?}"
    );
    assert_eq!(
        &argv[sep..],
        ["--", "-D", "warnings"],
        "only lint args after `--`: {argv:?}"
    );
    Ok(())
}

#[test]
fn clippy_featured_targeted_shape_is_exact() -> Result<(), ContractError> {
    let mut custom = group(TaskKind::Clippy);
    custom.features = vec!["serde".to_owned()];
    custom.target = "x86_64-unknown-linux-gnu".to_owned();
    assert_eq!(
        text(&custom)?,
        [
            "clippy",
            "--locked",
            "--offline",
            "--manifest-path",
            "Cargo.toml",
            "--package",
            "demo",
            "--all-targets",
            "--no-default-features",
            "--features",
            "serde",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--",
            "-D",
            "warnings",
        ]
    );
    Ok(())
}

#[test]
fn cargo_kinds_carry_features_without_separator() -> Result<(), ContractError> {
    for kind in [
        TaskKind::Test,
        TaskKind::Nextest,
        TaskKind::Doctest,
        TaskKind::Doc,
        TaskKind::Build,
    ] {
        let mut custom = group(kind);
        custom.features = vec!["serde".to_owned()];
        custom.target = "x86_64-unknown-linux-gnu".to_owned();
        let argv = text(&custom)?;
        assert!(
            !argv.contains(&"--".to_owned()),
            "{kind:?} emits no separator: {argv:?}"
        );
        assert!(
            argv.contains(&"--no-default-features".to_owned()),
            "{kind:?} keeps feature flags: {argv:?}"
        );
        assert!(
            argv.windows(2).any(|w| w == ["--features", "serde"]),
            "{kind:?} keeps feature list: {argv:?}"
        );
        assert!(
            argv.windows(2)
                .any(|w| w == ["--target", "x86_64-unknown-linux-gnu"]),
            "{kind:?} keeps target: {argv:?}"
        );
    }
    Ok(())
}

#[test]
fn payloads_never_emit_all_features() -> Result<(), ContractError> {
    let kinds = [
        TaskKind::Fmt,
        TaskKind::Clippy,
        TaskKind::Test,
        TaskKind::Nextest,
        TaskKind::Doctest,
        TaskKind::Doc,
        TaskKind::Build,
    ];
    for kind in kinds {
        for features in [
            vec!["default".to_owned()],
            vec!["serde".to_owned(), "cli".to_owned()],
            Vec::new(),
        ] {
            let mut group_case = group(kind);
            group_case.features = features;
            for arg in text(&group_case)? {
                assert!(
                    !arg.contains("all-features"),
                    "forbidden flag for {kind:?}: {arg}"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn entry_metadata_carries_driver_runner_and_evidence() {
    let group_case = group(TaskKind::Test);
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
    let mut configured = group(TaskKind::Nextest);
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
    let mut custom = group(TaskKind::Nextest);
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
    let mut package = group(TaskKind::Clippy);
    package.package_name = "-evil".to_owned();
    let err = cargo_payload_argv(&package).expect_err("dash package");
    assert!(err.to_string().contains("leading_dash_package"), "{err}");

    let mut manifest = group(TaskKind::Clippy);
    manifest.manifest_key = "-evil".to_owned();
    let err = cargo_payload_argv(&manifest).expect_err("dash manifest");
    assert!(err.to_string().contains("leading_dash_manifest"), "{err}");

    let mut features = group(TaskKind::Test);
    features.features = vec!["-evil".to_owned()];
    let err = cargo_payload_argv(&features).expect_err("dash features");
    assert!(err.to_string().contains("leading_dash_features"), "{err}");

    let mut target = group(TaskKind::Build);
    target.target = "--help".to_owned();
    let err = cargo_payload_argv(&target).expect_err("dash target");
    assert!(err.to_string().contains("leading_dash_target"), "{err}");

    let err = cargo_payload_with_profile(&target).expect_err("profiled dash target");
    assert!(err.to_string().contains("leading_dash_target"), "{err}");
}

#[test]
fn nextest_payload_carries_run_ignored() -> Result<(), ContractError> {
    let mut g = group(TaskKind::Nextest);
    g.test_runner = TestRunner::CargoNextest;
    g.run_ignored = Some("all".to_owned());
    let argv = text(&g)?;
    assert!(
        argv.windows(2).any(|w| w == ["--run-ignored", "all"]),
        "must contain --run-ignored all: {argv:?}"
    );

    let mut g_default = group(TaskKind::Nextest);
    g_default.test_runner = TestRunner::CargoNextest;
    g_default.run_ignored = Some("default".to_owned());
    let argv_default = text(&g_default)?;
    assert!(
        !argv_default.iter().any(|arg| arg == "--run-ignored"),
        "default mode must not emit --run-ignored: {argv_default:?}"
    );

    Ok(())
}
