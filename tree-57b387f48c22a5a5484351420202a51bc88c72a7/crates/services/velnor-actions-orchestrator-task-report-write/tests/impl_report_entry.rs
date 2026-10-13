//! Env-binding pins for the `write_task_report` entrypoint.
//!
//! The entrypoint resolves the run key from the GitHub environment
//! before the report-write core reads anything; every missing or
//! malformed binding fails closed with its exact taxonomy token, and
//! valid bindings delegate past run-key resolution. `unsafe_code =
//! "forbid"` bars in-process env mutation even in tests, so each case
//! re-executes this binary with a scrubbed child env (the
//! `without_ambient_identity` precedent in orchestrator-core): the
//! parent asserts exactly one passing child run, the child runs the
//! closure against its controlled env.

use velnor_actions_orchestrator_task_report_write::write_task_report;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const RUN_ID_ENV: &str = "GITHUB_RUN_ID";
const RUN_ATTEMPT_ENV: &str = "GITHUB_RUN_ATTEMPT";
const TEMP_ENV: &str = "RUNNER_TEMP";

/// Marker proving the child already runs under the controlled env.
const SCRUBBED_ENV: &str = "VELNOR_REPORT_ENTRY_SCRUBBED";

/// Run `inner` in a child holding exactly `keep` (bindings removed).
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

#[test]
fn missing_run_bindings_fail_with_missing_run_key() -> TestResult {
    with_child_env(
        "missing_run_bindings_fail_with_missing_run_key",
        &[],
        || {
            let err = write_task_report().expect_err("must fail closed");
            assert!(
                err.to_string().contains("missing_run_key"),
                "unexpected: {err}"
            );
            Ok(())
        },
    )
}

#[test]
fn non_numeric_run_id_fails_with_bad_run_id() -> TestResult {
    with_child_env(
        "non_numeric_run_id_fails_with_bad_run_id",
        &[(RUN_ID_ENV, "abc"), (RUN_ATTEMPT_ENV, "1")],
        || {
            let err = write_task_report().expect_err("must fail closed");
            assert!(err.to_string().contains("bad_run_id"), "unexpected: {err}");
            Ok(())
        },
    )
}

#[test]
fn non_numeric_attempt_fails_with_bad_run_attempt() -> TestResult {
    with_child_env(
        "non_numeric_attempt_fails_with_bad_run_attempt",
        &[(RUN_ID_ENV, "1"), (RUN_ATTEMPT_ENV, "xyz")],
        || {
            let err = write_task_report().expect_err("must fail closed");
            assert!(
                err.to_string().contains("bad_run_attempt"),
                "unexpected: {err}"
            );
            Ok(())
        },
    )
}

#[test]
fn valid_bindings_delegate_past_run_key() -> TestResult {
    with_child_env(
        "valid_bindings_delegate_past_run_key",
        &[(RUN_ID_ENV, "1"), (RUN_ATTEMPT_ENV, "1")],
        || {
            let err = write_task_report().expect_err("no plan staged");
            let text = err.to_string();
            for token in ["missing_run_key", "bad_run_id", "bad_run_attempt"] {
                assert!(!text.contains(token), "binding must succeed, got: {text}");
            }
            Ok(())
        },
    )
}
