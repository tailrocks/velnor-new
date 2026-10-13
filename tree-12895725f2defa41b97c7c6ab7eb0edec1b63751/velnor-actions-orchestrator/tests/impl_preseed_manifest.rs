//! F19 round-trip: fresh-binary manifest passes the kept-shell verifier.
//!
//! The writer (Rust) and the verifier (kept-shell exception) must agree
//! byte-for-byte. The `sh` spawn cannot live in `src/` (structural
//! scans forbid process-spawn tokens there, test modules included), so
//! it runs here: a scrubbed child re-exec drives the public env-driven
//! writer (child env needs no `unsafe`, which `unsafe_code = "forbid"`
//! bars even in tests), then the parent runs the real verifier script
//! over a real writer artifact plus a tampered twin.

use std::process::Command as StdCommand;

use tempfile::TempDir;

use crate::impl_common::TestResult;

/// Marker proving the child already carries writer env.
const CHILD_ENV: &str = "VELNOR_TEST_PRESEED_CHILD";

/// Full test path used as the child re-exec filter (unique substring).
const TEST_PATH: &str = "impl_preseed_manifest::fresh_binary_manifest_passes_shell_verifier";

/// Writer artifact passes the verifier; a tampered twin fails closed.
#[test]
#[cfg(unix)]
fn fresh_binary_manifest_passes_shell_verifier() -> TestResult {
    if std::env::var(CHILD_ENV).is_ok() {
        return velnor_actions_orchestrator::write_preseed_manifest()
            .map_err(|err| format!("child writer: {err}").into());
    }
    let tmp = TempDir::new()?;
    let root = tmp.path();
    let runner = root.display().to_string();
    let binary = root.join("velnor-actions");
    std::fs::write(&binary, "helper-bytes\n")?;
    let out = root.join("out").display().to_string();
    let target = "x86_64-unknown-linux-gnu";
    let commit = "c".repeat(40);
    let output = StdCommand::new(std::env::current_exe()?)
        .arg(TEST_PATH)
        .env(CHILD_ENV, "1")
        .env("RUNNER_TEMP", &runner)
        .env("GITHUB_SHA", &commit)
        .env("VELNOR_PRESEED_BINARY", binary.display().to_string())
        .env("VELNOR_PRESEED_OUT", &out)
        .env("VELNOR_PRESEED_TARGET", target)
        .env("VELNOR_PRESEED_TOOLCHAIN", "rust@1.98.1")
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "child: {stdout}{stderr}");
    assert!(stdout.contains("1 passed"), "child: {stdout}");
    // The verifier addresses the stage dir, not the output dir: relay
    // the artifact the way the upload/download round-trip would.
    let stage = root.join("velnor").join("preseed");
    std::fs::create_dir_all(&stage)?;
    std::fs::copy(
        root.join("out").join("velnor-actions"),
        stage.join("velnor-actions"),
    )?;
    std::fs::copy(
        root.join("out").join("preseed-manifest.json"),
        stage.join("preseed-manifest.json"),
    )?;
    let script = velnor_actions_workflow_jobs::preseed_manifest_verify_script(target);
    let run = || {
        StdCommand::new("sh")
            .args(["-c", &script])
            .env("RUNNER_TEMP", &runner)
            .env("GITHUB_SHA", &commit)
            .status()
            .map(|status| status.success())
    };
    assert!(run()?, "writer artifact must verify");
    std::fs::write(stage.join("velnor-actions"), "tampered-bytes\n")?;
    assert!(!run()?, "tampered binary must fail verification");
    Ok(())
}
