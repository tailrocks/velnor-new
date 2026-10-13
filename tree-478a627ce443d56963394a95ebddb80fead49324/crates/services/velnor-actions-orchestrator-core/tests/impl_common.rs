//! Fixtures shared by this target's core cases.
//!
//! Per-target copy of the orchestrator `impl_common` helpers (trimmed
//! to what this target uses): each integration target links alone, so
//! shared helpers ride per target.

use std::path::Path;
use std::process::Command as StdCommand;

/// Test error shortcut.
pub(crate) type TestResult = Result<(), Box<dyn std::error::Error>>;

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

/// Canonical-schema fixture for positive consumer-generation tests.
///
/// Its placeholder source and digests are serialization inputs, not
/// release provenance or qualification evidence.
pub(crate) fn fixture_manifest_json() -> String {
    include_str!("../../../../fixtures/consumer-release-manifest.json").to_owned()
}

/// Run git with inherited failure context.
pub(crate) fn git(args: &[&str], cwd: &Path) -> TestResult {
    let status = StdCommand::new("git")
        .args(args)
        .current_dir(cwd)
        .status()?;
    assert!(status.success(), "git {args:?} failed in {}", cwd.display());
    Ok(())
}
