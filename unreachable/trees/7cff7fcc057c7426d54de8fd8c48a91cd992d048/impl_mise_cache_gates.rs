//! Gate 4/6 cache cases: sources allowlist, task defs, modes, reuse.

use velnor_actions_contract::digest_b3;
use velnor_actions_mise::cache::{
    QualifiedTaskDef, TaskCacheMode, mode_for_event, qualify_reuse, save_allowed, task_run_argv,
    validate_sources_path, validate_task_def_path, verify_reused_outputs,
};
use velnor_actions_mise::restore_evidence::{RestoreObservation, classify_restore};
use velnor_actions_mise::{Gate6Fixture, render_gated_task_toml};

/// Fully observed restore: real path, bytes, and matching digests.
fn observed_restore() -> RestoreObservation {
    let bytes = b"entry bytes".to_vec();
    RestoreObservation {
        entry_path: "task-artifacts/v2/clippy/entry".to_owned(),
        entry_bytes: bytes.clone(),
        expected_digest: digest_b3(&bytes),
        expected_compat: digest_b3(b"compat"),
        observed_compat: digest_b3(b"compat"),
        expected_owner: "trusted".to_owned(),
        observed_owner: "trusted".to_owned(),
        expected_inputs: digest_b3(b"inputs"),
        observed_inputs: digest_b3(b"inputs"),
    }
}

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
    // T21: tofu init must-run carries the Network signal, so it can
    // never qualify for task-result reuse; validate's disablement is
    // plan/merge-time (forced execute + fail-closed reuse claims).
    assert!(qualify_reuse("init", true, false, false).is_err());
}

/// Restore attempts classify hits and precise miss reasons in order.
///
/// Classification consumes observed evidence only: a real observed
/// restore passes, and each unverified input fails with its reason.
#[test]
fn restore_classification_uses_precise_reasons() {
    assert!(classify_restore(&observed_restore()).is_ok());
    let check = |label: &str, obs: &RestoreObservation, reason: &str| {
        assert_eq!(classify_restore(obs), Err(reason), "{label}");
    };
    let mut obs = observed_restore();
    obs.entry_path.clear();
    check("missing entry", &obs, "no_entry");
    let mut obs = observed_restore();
    obs.entry_bytes = b"forged".to_vec();
    check("tampered bytes", &obs, "cache_corrupt");
    let mut obs = observed_restore();
    obs.observed_compat = digest_b3(b"other");
    check("compat drift", &obs, "compatibility_mismatch");
    let mut obs = observed_restore();
    obs.observed_owner = "pr".to_owned();
    check("owner drift", &obs, "trust_scope_mismatch");
    let mut obs = observed_restore();
    obs.observed_inputs = digest_b3(b"other");
    check("input drift", &obs, "input_digest_mismatch");
    let mut obs = observed_restore();
    obs.expected_inputs = "bogus".to_owned();
    check("malformed recorded digest", &obs, "input_digest_mismatch");
}

/// Every layer saves only producer-successful pushes.
///
/// Failed runs never save on any layer, and non-push events never save
/// through this path; unknown trust scopes deny closed.
#[test]
fn saves_gate_on_push_and_pass_for_all_layers() {
    for layer in ["trusted", "pr"] {
        assert!(save_allowed(layer, "push", true), "{layer} push saves");
        assert!(!save_allowed(layer, "push", false), "{layer} failed run");
        for event in [
            "pull_request",
            "merge_group",
            "fork",
            "release",
            "local",
            "schedule",
        ] {
            assert!(!save_allowed(layer, event, true), "{layer} {event}");
            assert!(!save_allowed(layer, event, false), "{layer} {event}");
        }
    }
    assert!(!save_allowed("unknown", "push", true));
    assert!(!save_allowed("", "push", true));
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

/// P04 zero-byte rule at the mise layer: an empty observation is
/// incomplete even when its digest verifies, matching the orchestrator.
#[test]
fn zero_byte_outputs_fail_mise_layer_as_incomplete() {
    let declared = vec!["out/report.json".to_owned()];
    let empty = vec![("out/report.json".to_owned(), Vec::new(), digest_b3(b""))];
    let err = verify_reused_outputs("clippy", &declared, &empty).expect_err("zero byte");
    assert!(
        err.to_string().contains("task_result_incomplete"),
        "P04 verdict: {err}"
    );
}
