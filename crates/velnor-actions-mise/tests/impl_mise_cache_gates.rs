//! Gate 4/6 cache cases: sources allowlist, task defs, modes, reuse.

use velnor_actions_contract::digest_b3;
use velnor_actions_mise::cache::{
    QualifiedTaskDef, TaskCacheMode, classify_restore, mode_for_event, qualify_reuse, save_allowed,
    task_run_argv, validate_sources_path, validate_task_def_path, verify_reused_outputs,
};
use velnor_actions_mise::{Gate6Fixture, render_gated_task_toml};

/// Sources allowlist accepts only `registry/` and `git/` subtrees.
#[test]
fn sources_allowlist_rejects_creds_targets_and_homes() {
    for ok in ["registry", "registry/cache/index", "git", "git/db/packs"] {
        assert!(validate_sources_path(ok).is_ok(), "{ok} must archive");
    }
    for bad in [
        "credentials",
        "credentials.toml",
        "registry/credentials",
        "target",
        "target/debug",
        ".rustup",
        "bin",
        "config.toml",
        "/abs/registry",
        "registry/../../etc",
        "",
    ] {
        assert!(
            validate_sources_path(bad).is_err(),
            "{bad} must be rejected"
        );
    }
}

/// Task-cache modes follow the event: local/pr/merge/push/release.
#[test]
fn modes_follow_event_kind() {
    assert_eq!(
        mode_for_event("local").expect("local"),
        TaskCacheMode::LocalOnly
    );
    assert_eq!(
        mode_for_event("pull_request").expect("pr"),
        TaskCacheMode::ReadOnly
    );
    assert_eq!(
        mode_for_event("merge_group").expect("merge"),
        TaskCacheMode::ReadOnly
    );
    assert_eq!(
        mode_for_event("push").expect("push"),
        TaskCacheMode::ReadWrite
    );
    assert_eq!(
        mode_for_event("release").expect("release"),
        TaskCacheMode::Off
    );
    assert!(mode_for_event("schedule").is_err());
}

/// Task definitions live under runner temp only, never `.mise/tasks`.
#[test]
fn task_defs_stay_under_runner_temp() {
    let good = "$RUNNER_TEMP/velnor/tasks/clippy.toml";
    assert!(validate_task_def_path(good).is_ok());
    for bad in [
        ".mise/tasks/clippy.toml",
        "mise.toml",
        "$RUNNER_TEMP/velnor/tasks/../evil.toml",
        "$RUNNER_TEMP/velnor/tasks/clippy.txt",
        "/tmp/tasks/clippy.toml",
        "tasks/clippy.toml",
    ] {
        assert!(
            validate_task_def_path(bad).is_err(),
            "{bad} must be rejected"
        );
    }
}

/// Rendered task TOML carries the version marker first plus fixed fields.
#[test]
fn task_toml_renders_marker_and_fixed_fields() {
    let def = QualifiedTaskDef {
        name: "clippy".to_owned(),
        run: vec!["cargo".to_owned(), "clippy".to_owned()],
        sources: vec!["src/**/*.rs".to_owned()],
        outputs: None,
        command_inputs: vec!["rustc --version".to_owned()],
    };
    let fixture = Gate6Fixture::new("gate6/cache-gates").expect("fixture");
    let toml = render_gated_task_toml("0.1.0", &def, &fixture).expect("render");
    let mut lines = toml.lines();
    assert_eq!(lines.next().expect("marker"), "# velnor-actions 0.1.0");
    assert!(
        toml.contains("outputs = []"),
        "empty outputs render: {toml}"
    );
    assert!(toml.contains("[cache]"), "cache section: {toml}");
    assert!(toml.contains("command_inputs"), "inputs: {toml}");
    let empty = QualifiedTaskDef {
        sources: Vec::new(),
        ..def.clone()
    };
    assert!(render_gated_task_toml("0.1.0", &empty, &fixture).is_err());
    let unnamed = QualifiedTaskDef {
        name: String::new(),
        ..def
    };
    assert!(render_gated_task_toml("0.1.0", &unnamed, &fixture).is_err());
}

/// Task invocation uses leading globals plus `--task-cache` and `--file`.
#[test]
fn task_argv_pins_globals_mode_and_file() {
    let argv = task_run_argv(
        TaskCacheMode::ReadOnly,
        "clippy",
        "$RUNNER_TEMP/velnor/tasks/clippy.toml",
    )
    .expect("argv");
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
    assert!(
        task_run_argv(
            TaskCacheMode::Off,
            "a b",
            "$RUNNER_TEMP/velnor/tasks/a.toml"
        )
        .is_err()
    );
    assert!(task_run_argv(TaskCacheMode::Off, "ok", ".mise/tasks/ok.toml").is_err());
}

/// Only deterministic tasks qualify; publish/deploy/notify/nondeterminism fail.
#[test]
fn reuse_qualification_rejects_nondeterministic_tasks() {
    assert!(qualify_reuse("clippy", false, false, false).is_ok());
    assert!(qualify_reuse("test", false, false, false).is_ok());
    for kind in ["publish", "deploy", "notify", "service"] {
        assert!(qualify_reuse(kind, false, false, false).is_err(), "{kind}");
    }
    assert!(qualify_reuse("clippy", true, false, false).is_err());
    assert!(qualify_reuse("clippy", false, true, false).is_err());
    assert!(qualify_reuse("clippy", false, false, true).is_err());
}

/// Restore attempts classify hits and precise miss reasons in order.
#[test]
fn restore_classification_uses_precise_reasons() {
    assert!(classify_restore([true, true, true, true, true]).is_ok());
    assert_eq!(
        classify_restore([false, true, true, true, true]),
        Err("no_entry")
    );
    assert_eq!(
        classify_restore([true, false, true, true, true]),
        Err("cache_corrupt")
    );
    assert_eq!(
        classify_restore([true, true, false, true, true]),
        Err("compatibility_mismatch")
    );
    assert_eq!(
        classify_restore([true, true, true, false, true]),
        Err("trust_scope_mismatch")
    );
    assert_eq!(
        classify_restore([true, true, true, true, false]),
        Err("input_digest_mismatch")
    );
}

/// Trusted layers save only on protected pushes with a pass.
#[test]
fn trusted_saves_gate_on_push_and_pass() {
    assert!(save_allowed("trusted", "push", true));
    assert!(!save_allowed("trusted", "push", false));
    assert!(!save_allowed("trusted", "pull_request", true));
    assert!(!save_allowed("trusted", "merge_group", true));
    assert!(save_allowed("pr", "pull_request", true));
}

/// Reuse requires every declared output present with a matching digest.
#[test]
fn reused_outputs_verify_presence_and_digests() {
    let bytes = b"report-bytes".to_vec();
    let digest = digest_b3(&bytes);
    let observed = vec![("out/report.json".to_owned(), bytes, digest)];
    let declared = vec!["out/report.json".to_owned()];
    assert!(verify_reused_outputs("clippy", &declared, &observed).is_ok());
    assert!(verify_reused_outputs("clippy", &["out/missing".to_owned()], &observed).is_err());
    let poisoned = vec![(
        "out/report.json".to_owned(),
        b"tampered".to_vec(),
        digest_b3(b"report-bytes"),
    )];
    assert!(verify_reused_outputs("clippy", &declared, &poisoned).is_err());
}
