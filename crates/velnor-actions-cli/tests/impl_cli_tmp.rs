//! Shared spawn and tempdir helpers for CLI integration tests.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

/// Monotonic counter keeping tempdir names unique within one test binary.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Create a fresh unique directory under the system temp dir.
pub(crate) fn fresh_tempdir(prefix: &str) -> Result<PathBuf, Box<dyn Error>> {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "velnor-cli-{prefix}-{}-{id}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Best-effort tempdir removal; cleanup must never fail a test.
pub(crate) fn cleanup(dir: &Path) {
    drop(std::fs::remove_dir_all(dir));
}

/// Spawn the `velnor-actions` binary with args, env, and cwd applied.
pub(crate) fn spawn(
    args: &[&str],
    env: &[(&str, &str)],
    cwd: &Path,
) -> Result<Output, Box<dyn Error>> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-actions"));
    command.args(args).current_dir(cwd);
    for (key, value) in env {
        command.env(key, value);
    }
    Ok(command.output()?)
}

/// Initialize a Git working tree in `dir`.
pub(crate) fn git_init(dir: &Path) -> Result<(), Box<dyn Error>> {
    let output = Command::new("git")
        .arg("init")
        .arg("-q")
        .arg(dir)
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("git init failed: {}", output.status).into())
    }
}

/// Exit code or -1 when the child died by signal.
pub(crate) fn code(output: &Output) -> i32 {
    output.status.code().unwrap_or(-1)
}
