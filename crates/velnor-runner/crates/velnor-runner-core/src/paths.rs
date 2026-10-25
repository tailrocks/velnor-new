pub const RUNNER_ROOT: &str = "/home/runner";

pub const RUNNER_WORK_FOLDER: &str = "_work";

#[must_use]
pub fn runner_work_path() -> String {
    format!("{RUNNER_ROOT}/{RUNNER_WORK_FOLDER}")
}
