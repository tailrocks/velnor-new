//! Cargo payload argv shape cases (moved from the orchestrator).
use velnor_actions_contract::ContractError;
use velnor_actions_rust::tasks::{
    TaskGroup, TaskKind, cargo_payload_argv, cargo_payload_with_profile,
};
use velnor_actions_rust::{CompileDriver, NextestProfile, TestRunner};

#[path = "impl_rust_argv_metadata.rs"]
mod metadata;

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
