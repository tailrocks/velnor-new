//! Ambient-environment helpers for orchestrator integration tests.

use super::TestResult;
use std::fs;
use std::process::Command as StdCommand;
use tempfile::TempDir;

/// Marker proving the current process already scrubbed ambient identity.
const SCRUBBED_ENV: &str = "VELNOR_TEST_SCRUBBED_IDENTITY";

/// True when ambient `GITHUB_REPOSITORY` names a non-canonical repo.
///
/// Fork CI exports the fork identity; Velnor-policy fixtures carry the
/// canonical origin, so an in-process `prepare` would fail closed on
/// the ambient hint instead of exercising the fixture.
pub(crate) fn ambient_identity_blocks() -> bool {
    std::env::var("GITHUB_REPOSITORY")
        .is_ok_and(|hint| hint.to_lowercase() != "tailrocks/velnor-new")
}

/// Run Velnor-policy assertions with ambient identity scrubbed.
///
/// When [`ambient_identity_blocks`], re-executes the calling test in a
/// child with `GITHUB_REPOSITORY` removed (child env needs no `unsafe`,
/// which `unsafe_code = "forbid"` bars even in tests) and requires
/// exactly one passing child run — a missing or failing child fails
/// loudly, never green. Otherwise runs `inner` in-process. `test` is
/// the bare test name, which must be unique in the binary.
pub(crate) fn without_ambient_identity(
    test: &str,
    inner: impl FnOnce() -> TestResult,
) -> TestResult {
    if std::env::var(SCRUBBED_ENV).is_err() && ambient_identity_blocks() {
        let output = StdCommand::new(std::env::current_exe()?)
            .arg(test)
            .env(SCRUBBED_ENV, "1")
            .env_remove("GITHUB_REPOSITORY")
            .output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{test}: {stdout}{stderr}");
        assert!(stdout.contains("1 passed"), "{test}: {stdout}");
        return Ok(());
    }
    inner()
}

/// Marker proving the current process already runs with the child event env.
const EVENT_ENV_MARKER: &str = "VELNOR_TEST_EVENT_ENV";

/// Run `inner` with `GITHUB_EVENT_PATH` set to a file holding `payload`, or
/// removed when `payload` is `None`, regardless of the ambient CI event.
///
/// Re-executes the calling test in a child with the environment adjusted
/// (child env needs no `unsafe`) and requires exactly one passing child run.
pub(crate) fn with_event_payload(
    test: &str,
    payload: Option<&str>,
    inner: impl FnOnce() -> TestResult,
) -> TestResult {
    if std::env::var(EVENT_ENV_MARKER).is_ok() {
        return inner();
    }
    let dir = TempDir::new()?;
    let mut child = StdCommand::new(std::env::current_exe()?);
    child.arg(test).env(EVENT_ENV_MARKER, "1");
    match payload {
        Some(payload) => {
            let path = dir.path().join("event.json");
            fs::write(&path, payload)?;
            child.env("GITHUB_EVENT_PATH", path);
        }
        None => {
            child.env_remove("GITHUB_EVENT_PATH");
        }
    }
    let output = child.output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{test}: {stdout}{stderr}");
    assert!(stdout.contains("1 passed"), "{test}: {stdout}");
    Ok(())
}
