//! Private gate for exact-head native Homebrew tap merge.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use velnor_actions_orchestrator::{OrchestratorError, merge_homebrew_tap_from_env};

const PLAN_FILE: &str = "package-release-plan.json";
const ROOT_DIR: &str = "velnor";

pub(super) fn gate_request(path: &Path) -> Option<PathBuf> {
    let runner_temp = PathBuf::from(env::var_os("RUNNER_TEMP").filter(|value| !value.is_empty())?);
    if !runner_temp.is_absolute() || !env::var("GITHUB_RUN_ID").is_ok_and(|value| !value.is_empty())
    {
        return None;
    }
    let root = runner_temp.join(ROOT_DIR);
    if path != root.join(PLAN_FILE)
        || env::var_os("VELNOR_REQUEST_FILE").as_deref() != Some(path.as_os_str())
        || !real_directory(&root)
        || !super::tap_prepare::package_input_available(&root)
        || !real_directory(&root.join("tap-candidate"))
        || !regular_file(&root.join("tap-candidate/formula.rb"))
        || !regular_file(&root.join("tap-candidate/candidate.json"))
        || !regular_file(&root.join("tap-pr.json"))
        || !regular_file(path)
    {
        return None;
    }
    for key in [
        "VELNOR_TAP_MERGE_TOKEN",
        "VELNOR_TAP_PROTECTION_TOKEN",
        "VELNOR_TAP_PROTECTION_APP_ID",
        "VELNOR_TAP_PROTECTION_APP_SLUG",
        "VELNOR_TAP_PROTECTION_INSTALLATION_ID",
        "VELNOR_TAP_APP_ID",
        "VELNOR_TAP_APP_SLUG",
        "VELNOR_TAP_INSTALLATION_ID",
        "VELNOR_TAP_COMPLETION_APP_ID",
        "VELNOR_TAP_REVIEWER_APP_ID_0",
        "VELNOR_TAP_POLICY",
        "GH_TOKEN",
    ] {
        if !env::var(key).is_ok_and(|value| !value.is_empty()) {
            return None;
        }
    }
    Some(path.to_path_buf())
}

pub(super) fn run() -> ExitCode {
    match merge_homebrew_tap_from_env() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail_error(&error),
    }
}

fn real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
}

fn regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| {
        meta.is_file()
            && !meta.file_type().is_symlink()
            && meta.len() > 0
            && meta.len() <= 1024 * 1024
    })
}

fn fail_error(error: &OrchestratorError) -> ExitCode {
    let message = error.to_string().replace(['\n', '\r'], " ");
    eprintln!("velnor-actions: internal request failed: {message}");
    ExitCode::from(1)
}
