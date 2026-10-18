use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use velnor_actions_orchestrator::cache_action_stage::{
    CACHE_TAR_LAUNCHER_INSTALL_OP, install_cache_tar_launchers,
};

pub(crate) const OP: &str = CACHE_TAR_LAUNCHER_INSTALL_OP;

/// Install cache-action tar aliases from the verified staged CLI.
pub(crate) fn run() -> ExitCode {
    if !super::cache_runner_context_is_valid() {
        return super::fail_internal("cache_runner_context_invalid");
    }
    let Some(runner_temp) = env::var_os("RUNNER_TEMP").filter(|value| !value.is_empty()) else {
        return super::fail_internal("cache_tar_launcher_runner_temp_missing");
    };
    let Some(version) = env::var("VELNOR_CACHE_TAR_VERSION")
        .ok()
        .filter(|value| !value.is_empty())
    else {
        return super::fail_internal("cache_tar_launcher_version_missing");
    };
    let Some(digest) = env::var("VELNOR_CACHE_TAR_SHA256")
        .ok()
        .filter(|value| !value.is_empty())
    else {
        return super::fail_internal("cache_tar_launcher_digest_missing");
    };
    match install_cache_tar_launchers(&PathBuf::from(runner_temp), &version, &digest) {
        Ok(_) => ExitCode::SUCCESS,
        Err(problem) => super::fail_internal(problem),
    }
}
