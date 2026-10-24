use std::env;
use std::path::{Path, PathBuf};
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
        Ok(alias_dir) => match append_alias_path(&alias_dir) {
            Ok(()) => ExitCode::SUCCESS,
            Err(problem) => super::fail_internal(problem),
        },
        Err(problem) => super::fail_internal(problem),
    }
}

fn append_alias_path(alias_dir: &Path) -> Result<(), &'static str> {
    let inherited = env::var_os("PATH")
        .filter(|value| !value.is_empty())
        .ok_or("cache_tar_launcher_inherited_path_missing")?;
    if !alias_dir
        .to_str()
        .is_some_and(|value| !value.is_empty() && !value.contains(':') && safe_single_line(value))
    {
        return Err("cache_tar_launcher_alias_path_invalid");
    }
    let inherited = inherited
        .to_str()
        .filter(|value| safe_single_line(value))
        .ok_or("cache_tar_launcher_inherited_path_invalid")?;
    let path_value = env::join_paths(
        std::iter::once(alias_dir.to_path_buf()).chain(env::split_paths(inherited)),
    )
    .map_err(|_| "cache_tar_launcher_path_join_failed")?;
    let path_value = path_value
        .to_str()
        .filter(|value| value.len() <= 32_768 && safe_single_line(value))
        .ok_or("cache_tar_launcher_path_output_invalid")?;
    let output = env::var_os(super::GITHUB_OUTPUT_ENV)
        .filter(|value| !value.is_empty())
        .ok_or("cache_tar_launcher_github_output_missing")?;
    let mut body = String::with_capacity(path_value.len() + 6);
    body.push_str("path=");
    body.push_str(path_value);
    body.push('\n');
    super::cache_bind::append_owned_output(&output, body.as_bytes())
        .map_err(|_| "cache_tar_launcher_output_write_failed")
}

fn safe_single_line(value: &str) -> bool {
    !value.is_empty()
        && !value
            .chars()
            .any(|character| character == '\0' || character == '\n' || character == '\r')
}
