//! Gate-6 enablement cases: qualified runs and fixture tokens.
use velnor_actions_mise::cache::{QualifiedTaskDef, TaskCacheMode};
use velnor_actions_mise::{
    Gate6Fixture, MiseError, qualified_task_run_argv, render_gated_task_toml,
};

fn sample_def() -> QualifiedTaskDef {
    QualifiedTaskDef {
        name: "clippy".to_owned(),
        run: vec!["cargo".to_owned(), "clippy".to_owned()],
        sources: vec!["src/**/*.rs".to_owned()],
        outputs: None,
        command_inputs: vec!["rustc --version".to_owned()],
    }
}

#[test]
fn qualified_argv_matches_fixed_shape() -> Result<(), String> {
    let argv = qualified_task_run_argv(
        "clippy",
        false,
        false,
        false,
        TaskCacheMode::ReadOnly,
        "clippy",
        "$RUNNER_TEMP/velnor/tasks/clippy.toml",
    )
    .map_err(|err| err.to_string())?;
    assert_eq!(
        argv,
        [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "run",
            "--task-cache",
            "read-only",
            "clippy",
            "--file",
            "$RUNNER_TEMP/velnor/tasks/clippy.toml",
        ]
    );
    Ok(())
}

#[test]
fn unqualified_tasks_never_reach_run_argv() {
    for kind in ["publish", "deploy", "notify", "service"] {
        assert!(
            qualified_task_run_argv(
                kind,
                false,
                false,
                false,
                TaskCacheMode::ReadOnly,
                kind,
                "$RUNNER_TEMP/velnor/tasks/task.toml",
            )
            .is_err(),
            "{kind} must not qualify"
        );
    }
    for (network, clock, random) in [
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        assert!(
            qualified_task_run_argv(
                "clippy",
                network,
                clock,
                random,
                TaskCacheMode::ReadOnly,
                "clippy",
                "$RUNNER_TEMP/velnor/tasks/clippy.toml",
            )
            .is_err(),
            "nondeterminism must not qualify"
        );
    }
}

#[test]
fn fixture_tokens_require_gate6_shape() -> Result<(), String> {
    let fixture = Gate6Fixture::new("gate6/clippy-cache").map_err(|err| err.to_string())?;
    assert_eq!(fixture.id(), "gate6/clippy-cache");
    for bad in [
        "",
        "clippy-cache",
        "gate5/clippy",
        "gate6/",
        "gate6/a/b",
        "gate6/has space",
        "gate6/has..dots",
    ] {
        assert!(
            matches!(
                Gate6Fixture::new(bad),
                Err(MiseError::CacheNotEligible { .. })
            ),
            "fixture must be rejected: {bad}"
        );
    }
    Ok(())
}

#[test]
fn gated_render_carries_marker_and_fixed_fields() -> Result<(), String> {
    let fixture = Gate6Fixture::new("gate6/clippy-cache").map_err(|err| err.to_string())?;
    let toml =
        render_gated_task_toml("0.1.0", &sample_def(), &fixture).map_err(|err| err.to_string())?;
    let mut lines = toml.lines();
    assert_eq!(lines.next().expect("marker"), "# velnor-actions 0.1.0");
    assert!(toml.contains("outputs = []"), "empty outputs: {toml}");
    assert!(toml.contains("[cache]"), "cache section: {toml}");
    let incomplete = QualifiedTaskDef {
        sources: Vec::new(),
        ..sample_def()
    };
    assert!(render_gated_task_toml("0.1.0", &incomplete, &fixture).is_err());
    Ok(())
}

#[test]
fn gated_render_configures_no_remote_cache() -> Result<(), String> {
    let fixture = Gate6Fixture::new("gate6/clippy-cache").map_err(|err| err.to_string())?;
    let toml =
        render_gated_task_toml("0.1.0", &sample_def(), &fixture).map_err(|err| err.to_string())?;
    for forbidden in [
        "remote_url",
        "remote-namespace",
        "namespace",
        "token",
        "oidc",
        "OIDC",
        "cache-server",
        "server",
    ] {
        assert!(
            !toml.contains(forbidden),
            "no remote cache surface: {forbidden} in {toml}"
        );
    }
    for mode in [
        TaskCacheMode::ReadWrite,
        TaskCacheMode::ReadOnly,
        TaskCacheMode::WriteOnly,
        TaskCacheMode::Off,
        TaskCacheMode::LocalOnly,
    ] {
        assert!(
            !mode.to_string().contains("remote"),
            "no remote mode: {mode}"
        );
    }
    Ok(())
}
