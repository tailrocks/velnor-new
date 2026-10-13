//! Env-binding pins for the `execute-check-v1` entrypoint.
//!
//! The entrypoint binds six check identity vars plus the run key before
//! the runtime executes anything; every missing or empty binding fails
//! closed with its exact taxonomy token. `unsafe_code = "forbid"` bars
//! in-process env mutation even in tests, so each case re-executes this
//! binary with a scrubbed child env (the
//! `without_ambient_identity` precedent in orchestrator-core): the
//! parent asserts exactly one passing child run, the child runs the
//! `inner` closure against its controlled env.

use std::process::Command;

use velnor_actions_contract_workflow::{NAMED_CHECK_JOB_ID_ENV, NAMED_CHECK_LANE_VARIANT_ENV};
use velnor_actions_orchestrator_check_runtime::{EXECUTE_CHECK_OP, execute_check};
use velnor_actions_orchestrator_core::report_keys::TASK_ID_ENV;
use velnor_actions_orchestrator_runtime_execute::execute::CHECK_ID_ENV;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const WORKSPACE_ENV: &str = "GITHUB_WORKSPACE";
const TEMP_ENV: &str = "RUNNER_TEMP";
const RUN_ID_ENV: &str = "GITHUB_RUN_ID";
const RUN_ATTEMPT_ENV: &str = "GITHUB_RUN_ATTEMPT";

/// Every var the entrypoint (plus run-key resolution) reads.
const ALL_KEYS: [&str; 8] = [
    WORKSPACE_ENV,
    TEMP_ENV,
    CHECK_ID_ENV,
    TASK_ID_ENV,
    NAMED_CHECK_JOB_ID_ENV,
    NAMED_CHECK_LANE_VARIANT_ENV,
    RUN_ID_ENV,
    RUN_ATTEMPT_ENV,
];

/// Marker proving the child already runs under the controlled env.
const SCRUBBED_ENV: &str = "VELNOR_CHECK_RUNTIME_SCRUBBED";

/// Six identity vars; the run key still resolves from the child env.
const IDENTITY: [(&str, &str); 6] = [
    (WORKSPACE_ENV, "/workspace"),
    (TEMP_ENV, "/tmp"),
    (CHECK_ID_ENV, "docker"),
    (TASK_ID_ENV, "stack/rust/demo/clippy/default"),
    (NAMED_CHECK_JOB_ID_ENV, "job-1"),
    (NAMED_CHECK_LANE_VARIANT_ENV, "hosted"),
];

/// Run `inner` in a child holding exactly `keep` (all other entrypoint
/// vars removed). `test` is the bare test name, unique in the binary.
fn with_child_env(
    test: &str,
    keep: &[(&str, &str)],
    inner: impl FnOnce() -> TestResult,
) -> TestResult {
    if std::env::var(SCRUBBED_ENV).is_err() {
        let mut child = Command::new(std::env::current_exe()?);
        child.arg(test).env(SCRUBBED_ENV, "1");
        for key in ALL_KEYS {
            child.env_remove(key);
        }
        for (key, value) in keep {
            child.env(key, value);
        }
        let output = child.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{test}: {stdout}{stderr}");
        assert!(stdout.contains("1 passed"), "{test}: {stdout}");
        return Ok(());
    }
    inner()
}

fn err_text() -> String {
    execute_check().expect_err("must fail closed").to_string()
}

#[test]
fn op_tag_is_execute_check_v1() {
    assert_eq!(EXECUTE_CHECK_OP, "execute-check-v1");
}

#[test]
fn missing_workspace_fails_closed() -> TestResult {
    with_child_env("missing_workspace_fails_closed", &[], || {
        let err = err_text();
        assert!(err.contains("missing_check_workspace"), "unexpected: {err}");
        Ok(())
    })
}

#[test]
fn missing_runner_temp_fails_closed() -> TestResult {
    with_child_env(
        "missing_runner_temp_fails_closed",
        &[(WORKSPACE_ENV, "/workspace")],
        || {
            let err = err_text();
            assert!(err.contains("missing_runner_temp"), "unexpected: {err}");
            Ok(())
        },
    )
}

#[test]
fn missing_check_id_fails_closed() -> TestResult {
    let keep: Vec<(&str, &str)> = IDENTITY
        .into_iter()
        .filter(|(key, _)| *key != CHECK_ID_ENV)
        .collect();
    with_child_env("missing_check_id_fails_closed", &keep, || {
        let err = err_text();
        assert!(err.contains("missing_check_identity"), "unexpected: {err}");
        Ok(())
    })
}

#[test]
fn missing_task_id_fails_closed() -> TestResult {
    let keep: Vec<(&str, &str)> = IDENTITY
        .into_iter()
        .filter(|(key, _)| *key != TASK_ID_ENV)
        .collect();
    with_child_env("missing_task_id_fails_closed", &keep, || {
        let err = err_text();
        assert!(err.contains("missing_check_identity"), "unexpected: {err}");
        Ok(())
    })
}

#[test]
fn missing_job_id_fails_closed() -> TestResult {
    let keep: Vec<(&str, &str)> = IDENTITY
        .into_iter()
        .filter(|(key, _)| *key != NAMED_CHECK_JOB_ID_ENV)
        .collect();
    with_child_env("missing_job_id_fails_closed", &keep, || {
        let err = err_text();
        assert!(err.contains("missing_check_identity"), "unexpected: {err}");
        Ok(())
    })
}

#[test]
fn missing_lane_variant_fails_closed() -> TestResult {
    let keep: Vec<(&str, &str)> = IDENTITY
        .into_iter()
        .filter(|(key, _)| *key != NAMED_CHECK_LANE_VARIANT_ENV)
        .collect();
    with_child_env("missing_lane_variant_fails_closed", &keep, || {
        let err = err_text();
        assert!(err.contains("missing_check_identity"), "unexpected: {err}");
        Ok(())
    })
}

#[test]
fn empty_values_rejected_like_missing() -> TestResult {
    let mut keep = IDENTITY.to_vec();
    keep[0] = (WORKSPACE_ENV, "");
    with_child_env("empty_values_rejected_like_missing", &keep, || {
        let err = err_text();
        assert!(err.contains("missing_check_workspace"), "unexpected: {err}");
        Ok(())
    })
}

#[test]
fn missing_run_env_fails_after_identity_binds() -> TestResult {
    with_child_env(
        "missing_run_env_fails_after_identity_binds",
        &IDENTITY,
        || {
            let err = err_text();
            assert!(err.contains("missing_run_key"), "unexpected: {err}");
            Ok(())
        },
    )
}

#[test]
fn malformed_run_attempt_rejected_before_execution() -> TestResult {
    let mut keep = IDENTITY.to_vec();
    keep.push((RUN_ID_ENV, "7"));
    keep.push((RUN_ATTEMPT_ENV, "first"));
    with_child_env(
        "malformed_run_attempt_rejected_before_execution",
        &keep,
        || {
            let err = err_text();
            assert!(err.contains("bad_run_attempt"), "unexpected: {err}");
            Ok(())
        },
    )
}
