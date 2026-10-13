//! Cargo payload argv shape cases (moved from the orchestrator).
use velnor_actions_rust::tasks::{
    TaskGroup, TaskKind, cargo_payload_argv, cargo_payload_with_profile, entry_metadata,
    evidence_id,
};
use velnor_actions_rust::{Evidence, EvidenceStrength, NextestProfile};

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
        compile_driver: "cargo".to_owned(),
        test_runner: "cargo_test".to_owned(),
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        nextest_profile: "default".to_owned(),
    }
}

fn text(group: &TaskGroup) -> Vec<String> {
    cargo_payload_argv(group)
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect()
}

/// Profiled payload argv under an explicit resolved profile.
fn profiled(group: &TaskGroup, profile: NextestProfile) -> Vec<String> {
    cargo_payload_with_profile(group, profile)
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn payload_shapes_per_kind() {
    assert_eq!(
        text(&group(TaskKind::Fmt))[..3],
        ["fmt", "--check", "--manifest-path"]
    );
    let mut workspace = group(TaskKind::Fmt);
    workspace.package_name.clear();
    assert_eq!(
        text(&workspace)[..4],
        ["fmt", "--all", "--check", "--manifest-path"]
    );
    let clippy = text(&group(TaskKind::Clippy));
    assert!(clippy.contains(&"--all-targets".to_owned()) && clippy.contains(&"demo".to_owned()));
    assert!(text(&group(TaskKind::Test)).contains(&"--lib".to_owned()));
    assert!(text(&group(TaskKind::Doctest)).contains(&"--doc".to_owned()));
}

#[test]
fn payload_features_and_target() {
    let mut custom = group(TaskKind::Test);
    custom.features = vec!["serde".to_owned(), "cli".to_owned()];
    custom.target = "x86_64-unknown-linux-gnu".to_owned();
    let argv = text(&custom);
    assert!(argv.contains(&"--no-default-features".to_owned()));
    assert!(
        argv.windows(2)
            .any(|w| w == ["--target", "x86_64-unknown-linux-gnu"])
    );
    assert!(!text(&group(TaskKind::Fmt)).contains(&"--no-default-features".to_owned()));
}

#[test]
fn clippy_keeps_feature_args_before_separator() {
    for features in [vec!["serde".to_owned(), "cli".to_owned()], Vec::new()] {
        let mut featured = group(TaskKind::Clippy);
        featured.features = features;
        let argv = text(&featured);
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
}

#[test]
fn clippy_keeps_target_before_separator() {
    let mut targeted = group(TaskKind::Clippy);
    targeted.target = "x86_64-unknown-linux-gnu".to_owned();
    let argv = text(&targeted);
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
}

#[test]
fn clippy_featured_targeted_shape_is_exact() {
    let mut custom = group(TaskKind::Clippy);
    custom.features = vec!["serde".to_owned()];
    custom.target = "x86_64-unknown-linux-gnu".to_owned();
    assert_eq!(
        text(&custom),
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
}

#[test]
fn cargo_kinds_carry_features_without_separator() {
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
        let argv = text(&custom);
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
}

#[test]
fn payloads_never_emit_all_features() {
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
            for arg in text(&group_case) {
                assert!(
                    !arg.contains("all-features"),
                    "forbidden flag for {kind:?}: {arg}"
                );
            }
        }
    }
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
    assert_eq!(metadata.compile_driver, "cargo");
    assert_eq!(metadata.test_runner, "cargo_test");
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
fn nextest_payload_is_pinned_tool_input() {
    let mut configured = group(TaskKind::Nextest);
    configured.nextest_profile = "ci".to_owned();
    let resolved = NextestProfile::parse(&configured.nextest_profile).expect("valid profile");
    assert_eq!(
        profiled(&configured, resolved),
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
    let resolved = NextestProfile::parse(&custom.nextest_profile).expect("valid profile");
    assert!(
        profiled(&custom, resolved)
            .windows(2)
            .any(|w| w == ["--manifest-path", "crates/demo/Cargo.toml"])
    );
    assert!(
        profiled(&custom, resolved)
            .windows(2)
            .any(|w| w == ["--profile", "default"])
    );
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
