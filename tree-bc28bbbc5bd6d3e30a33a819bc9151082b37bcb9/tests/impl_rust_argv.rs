//! Cargo payload argv shape cases (moved from the orchestrator).
use velnor_actions_rust::tasks::{TaskGroup, TaskKind, cargo_payload_argv};

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
    }
}

fn text(group: &TaskGroup) -> Vec<String> {
    cargo_payload_argv(group)
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
fn nextest_payload_is_pinned_tool_input() {
    assert_eq!(
        text(&group(TaskKind::Nextest)),
        [
            "nextest",
            "run",
            "--locked",
            "--offline",
            "--manifest-path",
            "Cargo.toml",
        ]
    );
    let mut custom = group(TaskKind::Nextest);
    custom.manifest_key = "crates/demo".to_owned();
    assert!(
        text(&custom)
            .windows(2)
            .any(|w| w == ["--manifest-path", "crates/demo/Cargo.toml"])
    );
}
