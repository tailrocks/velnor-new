//! Env-binding pins for the `fetch-reports-v1` entrypoint.
//!
//! The entrypoint binds the run ID, run key, and run directory from
//! the runner environment before the retrieve core counts anything;
//! every missing or malformed binding fails closed with its exact
//! taxonomy token. `unsafe_code = "forbid"` bars in-process env
//! mutation even in tests, so each case re-executes this binary with
//! a scrubbed child env (the `without_ambient_identity` precedent in
//! orchestrator-core): the parent asserts exactly one passing child
//! run, the child runs the closure against its controlled env.

use velnor_actions_orchestrator_retrieve_reports::{FETCH_OP, retrieve_reports};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const RUN_ID_ENV: &str = "GITHUB_RUN_ID";
const RUN_ATTEMPT_ENV: &str = "GITHUB_RUN_ATTEMPT";
const TEMP_ENV: &str = "RUNNER_TEMP";

/// Marker proving the child already runs under the controlled env.
const SCRUBBED_ENV: &str = "VELNOR_RETRIEVE_REPORTS_SCRUBBED";

/// Run `inner` in a child holding exactly `keep` (run bindings removed).
/// `test` is the bare test name, unique in the binary.
fn with_child_env(
    test: &str,
    keep: &[(&str, &str)],
    inner: impl FnOnce() -> TestResult,
) -> TestResult {
    if std::env::var(SCRUBBED_ENV).is_err() {
        let mut child = std::process::Command::new(std::env::current_exe()?);
        child.arg(test).env(SCRUBBED_ENV, "1");
        for key in [RUN_ID_ENV, RUN_ATTEMPT_ENV, TEMP_ENV] {
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
    retrieve_reports()
        .expect_err("must fail closed")
        .to_string()
}

#[test]
fn fetch_op_tag_is_stable() {
    assert_eq!(FETCH_OP, "fetch-reports-v1");
}

#[test]
fn missing_run_id_fails_closed() -> TestResult {
    with_child_env("missing_run_id_fails_closed", &[], || {
        let err = err_text();
        assert!(err.contains("missing_run_id"), "unexpected: {err}");
        Ok(())
    })
}

#[test]
fn empty_run_id_rejected_like_missing() -> TestResult {
    with_child_env(
        "empty_run_id_rejected_like_missing",
        &[(RUN_ID_ENV, "")],
        || {
            let err = err_text();
            assert!(err.contains("missing_run_id"), "unexpected: {err}");
            Ok(())
        },
    )
}

#[test]
fn non_numeric_run_id_rejected() -> TestResult {
    with_child_env(
        "non_numeric_run_id_rejected",
        &[(RUN_ID_ENV, "abc")],
        || {
            let err = err_text();
            assert!(err.contains("bad_run_id"), "unexpected: {err}");
            Ok(())
        },
    )
}

#[test]
fn missing_run_attempt_fails_closed() -> TestResult {
    with_child_env(
        "missing_run_attempt_fails_closed",
        &[(RUN_ID_ENV, "7")],
        || {
            let err = err_text();
            assert!(err.contains("missing_run_key"), "unexpected: {err}");
            Ok(())
        },
    )
}

#[test]
fn malformed_run_attempt_rejected() -> TestResult {
    with_child_env(
        "malformed_run_attempt_rejected",
        &[(RUN_ID_ENV, "7"), (RUN_ATTEMPT_ENV, "first")],
        || {
            let err = err_text();
            assert!(err.contains("bad_run_attempt"), "unexpected: {err}");
            Ok(())
        },
    )
}

#[test]
fn missing_runner_temp_fails_closed() -> TestResult {
    with_child_env(
        "missing_runner_temp_fails_closed",
        &[(RUN_ID_ENV, "7"), (RUN_ATTEMPT_ENV, "2")],
        || {
            let err = err_text();
            assert!(err.contains("missing_runner_temp"), "unexpected: {err}");
            Ok(())
        },
    )
}

#[test]
fn empty_runner_temp_rejected_like_missing() -> TestResult {
    with_child_env(
        "empty_runner_temp_rejected_like_missing",
        &[(RUN_ID_ENV, "7"), (RUN_ATTEMPT_ENV, "2"), (TEMP_ENV, "")],
        || {
            let err = err_text();
            assert!(err.contains("missing_runner_temp"), "unexpected: {err}");
            Ok(())
        },
    )
}

#[test]
fn empty_run_dir_retrieves_zero() -> TestResult {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().to_string_lossy().into_owned();
    with_child_env(
        "empty_run_dir_retrieves_zero",
        &[(RUN_ID_ENV, "7"), (RUN_ATTEMPT_ENV, "2"), (TEMP_ENV, &root)],
        || {
            assert_eq!(retrieve_reports().expect("count"), 0);
            Ok(())
        },
    )
}

#[test]
fn no_reports_dir_created_on_empty() -> TestResult {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().to_string_lossy().into_owned();
    with_child_env(
        "no_reports_dir_created_on_empty",
        &[(RUN_ID_ENV, "7"), (RUN_ATTEMPT_ENV, "2"), (TEMP_ENV, &root)],
        || {
            retrieve_reports().expect("count");
            let root = std::env::var(TEMP_ENV).expect("child temp");
            assert!(
                !std::path::Path::new(&root)
                    .join("velnor")
                    .join("r7-a2")
                    .join("reports")
                    .exists(),
                "no downloads attempted"
            );
            Ok(())
        },
    )
}
