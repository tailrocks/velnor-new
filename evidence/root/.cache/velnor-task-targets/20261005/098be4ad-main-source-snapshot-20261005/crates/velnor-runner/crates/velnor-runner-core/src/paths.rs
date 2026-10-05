//! Shared filesystem paths for the runner image contract.

/// Runner installation root inside the runner image.
pub const RUNNER_ROOT: &str = "/home/runner";

/// The relative path the Actions runner resolves beneath its installation root.
pub const RUNNER_WORK_FOLDER: &str = "_work";

/// Build the absolute working directory consumed by JIT and container mounts.
#[must_use]
pub fn runner_work_path() -> String {
    format!("{RUNNER_ROOT}/{RUNNER_WORK_FOLDER}")
}
