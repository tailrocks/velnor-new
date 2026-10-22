//! Private native package-release operation admission and signer dispatch.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use velnor_actions_orchestrator::{OrchestratorError, sign_package_release_from_env};

const RUNNER_TEMP_ENV: &str = "RUNNER_TEMP";
const REQUEST_FILE: &str = "package-release-policy.json";

/// Validate the exact signer request location and its regular-file identity.
pub(super) fn gate_request(path: &Path) -> Option<PathBuf> {
    gate_named_request(path, REQUEST_FILE)
}
pub(super) fn gate_producer_request(path: &Path) -> Option<PathBuf> {
    gate_named_request(path, "package-release-plan.json")
}
pub(super) fn gate_preview_source_seed_request(path: &Path) -> Option<PathBuf> {
    gate_named_request(path, "preview-source-seed-policy.json")
}
pub(super) fn gate_preview_source_acquisition(path: &Path) -> Option<PathBuf> {
    let runner_temp = env::var_os(RUNNER_TEMP_ENV).filter(|value| !value.is_empty())?;
    let root = PathBuf::from(runner_temp).join("velnor");
    if !root.is_absolute() || path != root.join(REQUEST_FILE) {
        return None;
    }
    let root_meta = fs::symlink_metadata(&root).ok()?;
    if root_meta.file_type().is_symlink() || !root_meta.is_dir() {
        return None;
    }
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        _ => return None,
    }
    Some(path.to_path_buf())
}
fn gate_named_request(path: &Path, name: &str) -> Option<PathBuf> {
    let runner_temp = env::var_os(RUNNER_TEMP_ENV).filter(|value| !value.is_empty())?;
    let runner_temp = PathBuf::from(runner_temp);
    if !runner_temp.is_absolute() {
        return None;
    }
    let root = runner_temp.join("velnor");
    let expected = root.join(name);
    if path != expected {
        return None;
    }
    let root_meta = fs::symlink_metadata(&root).ok()?;
    if root_meta.file_type().is_symlink() || !root_meta.is_dir() {
        return None;
    }
    let request_meta = fs::symlink_metadata(path).ok()?;
    if request_meta.file_type().is_symlink()
        || !request_meta.is_file()
        || request_meta.len() == 0
        || request_meta.len() > 1024 * 1024
    {
        return None;
    }
    Some(path.to_path_buf())
}

/// Run the Velnor-owned signer after the private operation gate succeeds.
pub(super) fn run() -> ExitCode {
    match sign_package_release_from_env() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail_internal(&error),
    }
}

fn fail_internal(error: &OrchestratorError) -> ExitCode {
    let message = error.to_string().replace(['\n', '\r'], " ");
    eprintln!("velnor-actions: internal request failed: {message}");
    ExitCode::from(1)
}
